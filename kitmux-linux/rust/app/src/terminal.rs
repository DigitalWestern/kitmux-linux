use crate::ffi::{KitmuxGdkKeyInput, KitmuxKeyTracker, KitmuxKeyTranslation};
use gtk::gdk;
use gtk::glib::{self};
use gtk::prelude::*;
use gtk::{ApplicationWindow, GLArea, Label};
use kitmux_model::{
    AppModel, AppSnapshot, CloseOutcome, CommandId, ControlEventHistory, ControlServer,
    DEFAULT_WHEEL_SCROLL_LINES, Direction, GroupId, GroupModel, LoadDisposition, NavigationTarget,
    PaneContainer, PaneContentKind, PaneDetail, PaneId, PaneRuntime, PaneSurface,
    PaneSurfaceDetail, PixelRect, PixelSize, PollingFileWatcher, RestoreLayoutPolicy,
    ResumeCommandCurrentState, ResumeCommandSelectionPolicy, SETTINGS_MAX_BYTES, SNAPSHOT_VERSION,
    SettingsDocument, ShortcutAction, ShortcutChord, ShortcutMap, SplitAxis, SplitId, SplitLayout,
    SshProfile, SshProfileStore, SshResolution, SurfaceId, TabGroupSnapshot, TabId, TabModel,
    TerminalRuntime, TerminalTabSnapshot, WorkspaceId, WorkspaceModel, WorkspaceSnapshot, XdgPaths,
    detected_url, load_settings_at_launch, load_state_at_launch, namespaced_number_target,
    reload_settings, save_state, terminal_cell_scaled, valid_resume_command,
};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::ffi::{CStr, CString, OsString, c_char, c_int, c_void};
use std::ops::{Deref, DerefMut};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::rc::{Rc, Weak};
use std::sync::atomic::Ordering;
use std::time::Duration;
use uuid::Uuid;

use crate::TERMINATION_REQUESTED;
use crate::control::CONTROL_WAKE;
use crate::diagnostic;
use crate::ffi;
use crate::menu::set_menu_accelerators;
use crate::navigation::{ForegroundScope, NavigationEffect, NavigationUi, RenameTarget, changed};
use crate::restore::{ResumeOffer, restored_product};
use crate::runtime::{
    RuntimeBundle, account, c_buffer, kitty_stage_error, owned_c_string, path_cstring,
    session_environment, valid_restored_cwd,
};
use crate::ssh::disconnected_ssh_argv;

pub(crate) const PTY_SOURCE_PRIORITY: c_int = 200;
pub(crate) const G_IO_IN: u32 = 1;
pub(crate) const G_IO_ERR: u32 = 8;
pub(crate) const G_IO_HUP: u32 = 16;
pub(crate) const G_IO_NVAL: u32 = 32;
pub(crate) const SPLIT_GAP: i32 = 4;
pub(crate) const MINIMUM_PANE: PixelSize = PixelSize::new(80, 50);

#[repr(C)]
pub(crate) struct TerminalRegion {
    pub(crate) session: *mut ffi::KittySession,
    pub(crate) x: c_int,
    pub(crate) y: c_int,
    pub(crate) width: c_int,
    pub(crate) height: c_int,
    pub(crate) previous_width: c_int,
    pub(crate) previous_height: c_int,
    pub(crate) viewport_changed: bool,
}

unsafe extern "C" {
    pub(crate) fn g_unix_fd_add_full(
        priority: c_int,
        fd: c_int,
        condition: u32,
        callback: Option<unsafe extern "C" fn(c_int, u32, *mut c_void) -> c_int>,
        userdata: *mut c_void,
        destroy: Option<unsafe extern "C" fn(*mut c_void)>,
    ) -> u32;
    pub(crate) fn g_source_remove(source: u32) -> c_int;
    pub(crate) fn kitmux_terminal_render_regions(
        engine: *mut ffi::KittyEngine,
        buffer_scale: f64,
        regions: *mut TerminalRegion,
        region_count: usize,
        cell_width: *mut c_int,
        cell_height: *mut c_int,
        error: *mut c_char,
        error_len: usize,
    ) -> bool;
}

pub(crate) fn load_disposition_name(disposition: &LoadDisposition) -> &'static str {
    match disposition {
        LoadDisposition::Missing => "missing",
        LoadDisposition::Loaded => "loaded",
        LoadDisposition::SetAside(_) => "set-aside",
        LoadDisposition::RecoveredFromLastGood => "last-good",
        LoadDisposition::Unreadable => "unreadable",
    }
}

pub(crate) struct CallbackUi {
    pub(crate) window: glib::WeakRef<ApplicationWindow>,
    pub(crate) area: glib::WeakRef<GLArea>,
    pub(crate) status: glib::WeakRef<Label>,
    pub(crate) visible: Cell<bool>,
    pub(crate) disconnected: Cell<bool>,
    pub(crate) close_window_on_exit: bool,
    pub(crate) pending_resume_command: RefCell<Option<Option<String>>>,
}

unsafe extern "C" fn on_damage(userdata: *mut c_void) {
    let ui = unsafe { &*(userdata.cast::<CallbackUi>()) };
    if ui.visible.get()
        && let Some(area) = ui.area.upgrade()
    {
        area.queue_render();
    }
}

unsafe extern "C" fn on_title(userdata: *mut c_void, title: *const c_char) {
    if title.is_null() {
        return;
    }
    let ui = unsafe { &*(userdata.cast::<CallbackUi>()) };
    let title = unsafe { CStr::from_ptr(title) }
        .to_string_lossy()
        .chars()
        .filter(|character| !character.is_control())
        .take(256)
        .collect::<String>();
    let characters = title.chars().count();
    if ui.visible.get()
        && !title.is_empty()
        && let Some(window) = ui.window.upgrade()
    {
        window.set_title(Some(&title));
        diagnostic("title_updated", &[("characters", characters.to_string())]);
    }
}

unsafe extern "C" fn on_bell(userdata: *mut c_void) {
    let ui = unsafe { &*(userdata.cast::<CallbackUi>()) };
    if ui.visible.get()
        && let Some(area) = ui.area.upgrade()
    {
        area.error_bell();
    }
}

unsafe extern "C" fn on_child_exit(userdata: *mut c_void, status: c_int) {
    let ui = unsafe { &*(userdata.cast::<CallbackUi>()) };
    ui.disconnected.set(true);
    if ui.visible.get()
        && let Some(label) = ui.status.upgrade()
    {
        label.set_text(&format!("Shell exited with status {status}"));
    }
    diagnostic("child_exit", &[("status", status.to_string())]);
    if ui.close_window_on_exit
        && ui.visible.get()
        && let Some(window) = ui.window.upgrade()
    {
        glib::idle_add_local_once(move || window.close());
    }
}

unsafe extern "C" fn on_user_var(userdata: *mut c_void, key: *const c_char, value: *const c_char) {
    if key.is_null() || value.is_null() {
        return;
    }
    let ui = unsafe { &*(userdata.cast::<CallbackUi>()) };
    let key = unsafe { CStr::from_ptr(key) };
    if key.to_bytes() != b"kitmux_resume" {
        return;
    }
    let value = unsafe { CStr::from_ptr(value) }.to_string_lossy();
    let command = valid_resume_command((!value.is_empty()).then_some(value.as_ref()));
    *ui.pending_resume_command.borrow_mut() = Some(command);
}

pub(crate) struct SessionState {
    pub(crate) session: *mut ffi::KittySession,
    pub(crate) callback_ui: Option<Box<CallbackUi>>,
    pub(crate) pty_source: u32,
    pub(crate) framebuffer_width: c_int,
    pub(crate) framebuffer_height: c_int,
    pub(crate) cell_width: c_int,
    pub(crate) cell_height: c_int,
    pub(crate) last_cwd: Option<PathBuf>,
    pub(crate) keys: KitmuxKeyTracker,
    pub(crate) im_consumed: KitmuxKeyTracker,
    pub(crate) preedit_active: bool,
    pub(crate) filtering: bool,
    pub(crate) filtering_had_preedit: bool,
    pub(crate) filtering_committed: bool,
    pub(crate) filtering_encoded: bool,
    pub(crate) filtering_input: KitmuxGdkKeyInput,
    pub(crate) scroll_residue: f64,
    pub(crate) selection_active: bool,
    pub(crate) mouse_reporting_button: Option<c_int>,
    pub(crate) hidden_pump_reported: bool,
    pub(crate) ssh_profile_id: Option<Uuid>,
    pub(crate) resume_command: Option<String>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            session: ptr::null_mut(),
            callback_ui: None,
            pty_source: 0,
            framebuffer_width: 0,
            framebuffer_height: 0,
            cell_width: 0,
            cell_height: 0,
            last_cwd: None,
            keys: KitmuxKeyTracker::default(),
            im_consumed: KitmuxKeyTracker::default(),
            preedit_active: false,
            filtering: false,
            filtering_had_preedit: false,
            filtering_committed: false,
            filtering_encoded: false,
            filtering_input: KitmuxGdkKeyInput::default(),
            scroll_residue: 0.0,
            selection_active: false,
            mouse_reporting_button: None,
            hidden_pump_reported: false,
            ssh_profile_id: None,
            resume_command: None,
        }
    }
}

pub(crate) struct Terminal {
    pub(crate) engine: *mut ffi::KittyEngine,
    pub(crate) sessions: HashMap<SurfaceId, SessionState>,
    pub(crate) active_surface_id: SurfaceId,
    pub(crate) xdg: Option<XdgPaths>,
    pub(crate) shortcuts: ShortcutMap,
    pub(crate) default_font_size: f64,
    pub(crate) shortcut_consumed: KitmuxKeyTracker,
    pub(crate) close_confirmed: bool,
    pub(crate) fullscreen: bool,
    pub(crate) zoomed_pane: Option<PaneId>,
    pub(crate) modal_dialog_open: bool,
    pub(crate) paste_confirmation_threshold: usize,
    pub(crate) wheel_scroll_lines: u64,
    pub(crate) confirm_close_with_running_process: bool,
    pub(crate) persistence: Option<PersistenceState>,
    pub(crate) settings_source: Option<glib::SourceId>,
    pub(crate) account_home: PathBuf,
    pub(crate) workspace_id: WorkspaceId,
    pub(crate) pane_id: PaneId,
    pub(crate) navigation: Option<AppModel>,
    pub(crate) navigation_ui: Option<NavigationUi>,
    pub(crate) created_workspaces: usize,
    pub(crate) created_groups: usize,
    pub(crate) divider_drag: Option<(SplitId, f64, f64)>,
    pub(crate) control_server: Option<ControlServer>,
    pub(crate) control_notice: Option<String>,
    pub(crate) control_history: ControlEventHistory,
    pub(crate) ssh_profiles: Option<SshProfileStore>,
    pub(crate) pending_resume_offers: Vec<ResumeOffer>,
}

pub(crate) struct PendingTerminalRuntime {
    pub(crate) closed: bool,
}

impl TerminalRuntime for PendingTerminalRuntime {
    fn pump(&mut self) {}

    fn close(&mut self) {
        self.closed = true;
    }

    fn is_closed(&self) -> bool {
        self.closed
    }
}

pub(crate) struct PersistenceState {
    pub(crate) state_path: PathBuf,
    pub(crate) state_may_write: bool,
    pub(crate) settings_path: PathBuf,
    pub(crate) settings: SettingsDocument,
    pub(crate) settings_watcher: Option<PollingFileWatcher>,
}

impl Default for Terminal {
    fn default() -> Self {
        let active_surface_id = SurfaceId::new();
        Self {
            engine: ptr::null_mut(),
            sessions: HashMap::from([(active_surface_id, SessionState::default())]),
            active_surface_id,
            xdg: None,
            shortcuts: ShortcutMap::linux_defaults(),
            default_font_size: 0.0,
            shortcut_consumed: KitmuxKeyTracker::default(),
            close_confirmed: false,
            fullscreen: false,
            zoomed_pane: None,
            modal_dialog_open: false,
            paste_confirmation_threshold: 8192,
            wheel_scroll_lines: DEFAULT_WHEEL_SCROLL_LINES,
            confirm_close_with_running_process: false,
            persistence: None,
            settings_source: None,
            account_home: PathBuf::new(),
            workspace_id: WorkspaceId::new(),
            pane_id: PaneId::new(),
            navigation: None,
            navigation_ui: None,
            created_workspaces: 1,
            created_groups: 1,
            divider_drag: None,
            control_server: None,
            control_notice: None,
            control_history: ControlEventHistory::default(),
            ssh_profiles: None,
            pending_resume_offers: Vec::new(),
        }
    }
}

impl Deref for Terminal {
    type Target = SessionState;

    fn deref(&self) -> &Self::Target {
        &self.sessions[&self.active_surface_id]
    }
}

impl DerefMut for Terminal {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.sessions
            .get_mut(&self.active_surface_id)
            .expect("active surface session exists")
    }
}

pub(crate) fn split_geometry(area: &GLArea) -> (PixelRect, i32, PixelSize) {
    let factor = area.scale_factor().max(1);
    (
        PixelRect::new(
            0,
            0,
            area.width().max(1) * factor,
            area.height().max(1) * factor,
        ),
        SPLIT_GAP * factor,
        MINIMUM_PANE,
    )
}

pub(crate) fn rect_contains(rect: PixelRect, x: f64, y: f64) -> bool {
    x >= f64::from(rect.x)
        && x < f64::from(rect.x + rect.width)
        && y >= f64::from(rect.y)
        && y < f64::from(rect.y + rect.height)
}

pub(crate) fn pending_pane(id: PaneId, surface_id: SurfaceId) -> PaneContainer {
    PaneContainer::new(
        id,
        vec![PaneSurface::new(
            surface_id,
            PaneRuntime::Terminal(Box::new(PendingTerminalRuntime { closed: false })),
        )],
        0,
    )
    .expect("a one-surface pane is valid")
}

pub(crate) fn pending_tab() -> (TabModel, SurfaceId) {
    let pane = PaneId::new();
    let surface = SurfaceId::new();
    (
        TabModel::single(TabId::new(), pending_pane(pane, surface)),
        surface,
    )
}

pub(crate) fn pending_group() -> (GroupModel, SurfaceId) {
    let (tab, surface) = pending_tab();
    (GroupModel::single(GroupId::new(), tab), surface)
}

pub(crate) fn pending_workspace() -> (WorkspaceModel, SurfaceId) {
    let (group, surface) = pending_group();
    (WorkspaceModel::single(WorkspaceId::new(), group), surface)
}

pub(crate) fn initial_navigation(
    workspace: WorkspaceId,
    pane: PaneId,
    surface: SurfaceId,
) -> AppModel {
    AppModel::single(WorkspaceModel::single(
        workspace,
        GroupModel::single(
            GroupId::new(),
            TabModel::single(TabId::new(), pending_pane(pane, surface)),
        ),
    ))
}

impl Terminal {
    pub(crate) fn initialize(
        &mut self,
        area: &GLArea,
        window: &ApplicationWindow,
        status: &Label,
    ) -> Result<c_int, String> {
        if !self.engine.is_null() {
            return Ok(unsafe { ffi::kitty_session_fd(self.session) });
        }
        area.make_current();
        if area.error().is_some() {
            return Err("opengl-context".to_owned());
        }
        let runtime = RuntimeBundle::discover().map_err(str::to_owned)?;
        let account = account();
        let environment: HashMap<String, String> = env::vars_os()
            .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
            .collect();
        let xdg =
            XdgPaths::resolve(&environment, &account.home).map_err(|_| "xdg-paths".to_owned())?;
        let settings_path = xdg.settings_file();
        let state_path = xdg.state_file();
        let settings_load = load_settings_at_launch(&settings_path);
        let state_load = load_state_at_launch(&state_path);
        self.shortcuts = ShortcutMap::linux_from_settings(&settings_load.document);
        if let Some(app) = self.navigation_ui.as_ref().and_then(|ui| ui.app.upgrade()) {
            set_menu_accelerators(&app, &self.shortcuts);
        }
        self.paste_confirmation_threshold = usize::try_from(
            settings_load
                .document
                .resolved()
                .paste_confirmation_threshold_bytes,
        )
        .unwrap_or(usize::MAX);
        self.wheel_scroll_lines = settings_load.document.wheel_scroll_lines();
        self.confirm_close_with_running_process = settings_load
            .document
            .resolved()
            .confirm_close_with_running_process;
        let menu_bar_visible = settings_load.document.resolved().menu_bar_visible_on_launch;
        window.set_show_menubar(menu_bar_visible);
        if let Some(menu_bar) = self
            .navigation_ui
            .as_ref()
            .and_then(|ui| ui.menu_bar.upgrade())
        {
            menu_bar.set_visible(menu_bar_visible);
        }
        if let Some(sidebar) = self
            .navigation_ui
            .as_ref()
            .and_then(|ui| ui.sidebar_shell.upgrade())
        {
            let resolved = settings_load.document.resolved();
            sidebar.set_visible(resolved.sidebar_visible_on_launch);
            sidebar.set_width_request(resolved.sidebar_width_points as i32);
        }
        let restored_font = state_load
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.font_size);
        let restored = (settings_load.document.resolved().restore_layout
            == RestoreLayoutPolicy::Always)
            .then(|| {
                state_load
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| restored_product(snapshot, &account.home))
            })
            .flatten();
        let restored_layout = restored.is_some();
        self.account_home = account.home.clone();
        let (
            navigation,
            surface_cwds,
            valid_restored_cwds,
            surface_resume_commands,
            surface_ssh_profiles,
            resume_offers,
        ) = if let Some(restored) = restored {
            self.active_surface_id = restored.active_surface;
            self.sessions = HashMap::from([(restored.active_surface, SessionState::default())]);
            self.created_workspaces = restored.created_workspaces;
            self.created_groups = restored.created_groups;
            (
                restored.navigation,
                restored.surface_cwds,
                restored.valid_restored_cwds,
                restored.surface_resume_commands,
                restored.surface_ssh_profiles,
                restored.resume_offers,
            )
        } else {
            let workspace = WorkspaceId::new();
            let pane = PaneId::new();
            (
                initial_navigation(workspace, pane, self.active_surface_id),
                HashMap::from([(self.active_surface_id, account.home.clone())]),
                HashMap::new(),
                HashMap::new(),
                HashMap::new(),
                Vec::new(),
            )
        };
        self.pending_resume_offers = resume_offers;
        let restored_resume_command = surface_resume_commands
            .get(&self.active_surface_id)
            .cloned();
        let restored_ssh_profile = surface_ssh_profiles.get(&self.active_surface_id).copied();
        let restored_cwd = valid_restored_cwds.get(&self.active_surface_id).cloned();
        self.last_cwd = restored_cwd.clone();
        if self.last_cwd.is_some() {
            diagnostic("cwd_restore_seeded", &[]);
        }
        self.workspace_id = navigation.active_workspace().id();
        self.pane_id = navigation.active_tab().focused_pane_id();
        self.navigation = Some(navigation);
        self.persistence = Some(PersistenceState {
            state_path,
            state_may_write: state_load.may_write,
            settings_watcher: PollingFileWatcher::new(
                settings_path.clone(),
                SETTINGS_MAX_BYTES as u64,
            )
            .ok(),
            settings_path,
            settings: settings_load.document,
        });
        let ssh_profile_path = env::var_os("KITMUX_SSH_PROFILES_PATH")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| xdg.ssh_profiles_file());
        self.ssh_profiles = match SshProfileStore::open(ssh_profile_path) {
            Ok(store) => Some(store),
            Err(error) => {
                diagnostic("ssh_store_unavailable", &[("reason", error.to_string())]);
                None
            }
        };
        self.xdg = Some(xdg);
        diagnostic(
            "persistence_loaded",
            &[
                (
                    "settings",
                    load_disposition_name(&settings_load.disposition).to_owned(),
                ),
                (
                    "state",
                    load_disposition_name(&state_load.disposition).to_owned(),
                ),
                ("cwd", restored_cwd.is_some().to_string()),
                ("font", restored_font.is_some().to_string()),
            ],
        );
        if restored_layout && let Some(navigation) = self.navigation.as_ref() {
            diagnostic(
                "hierarchy_restored",
                &[
                    ("workspaces", navigation.workspaces().len().to_string()),
                    ("sessions", surface_cwds.len().to_string()),
                ],
            );
        }

        let config = ffi::KittyEngineConfig {
            kitty_src_path: runtime.kitty_src.as_ptr(),
            libkitty_py_path: runtime.libkitty_py.as_ptr(),
            python_home: runtime.python_home.as_ptr(),
            config_path: runtime
                .config
                .as_ref()
                .map_or(ptr::null(), |value| value.as_ptr()),
        };
        let mut error = [0 as c_char; 1024];
        let engine = unsafe { ffi::kitty_engine_init(&config, error.as_mut_ptr(), error.len()) };
        if engine.is_null() {
            return Err(kitty_stage_error("engine-init", &error));
        }
        let scale = f64::from(area.scale_factor());
        if !unsafe {
            ffi::kitty_render_init(
                engine,
                scale,
                &mut self.cell_width,
                &mut self.cell_height,
                error.as_mut_ptr(),
                error.len(),
            )
        } {
            unsafe { ffi::kitty_engine_shutdown(engine) };
            return Err(kitty_stage_error("renderer-init", &error));
        }
        self.default_font_size = unsafe { ffi::kitty_render_font_size(engine) };
        if let Some(points) = restored_font {
            if !unsafe {
                ffi::kitty_render_set_font_size(
                    engine,
                    points,
                    &mut self.cell_width,
                    &mut self.cell_height,
                    error.as_mut_ptr(),
                    error.len(),
                )
            } {
                unsafe { ffi::kitty_engine_shutdown(engine) };
                return Err(kitty_stage_error("restored-font", &error));
            }
            diagnostic("font_restored", &[("points", format!("{points:.2}"))]);
        }

        let mut callback_ui = Box::new(CallbackUi {
            window: window.downgrade(),
            area: area.downgrade(),
            status: status.downgrade(),
            visible: Cell::new(true),
            disconnected: Cell::new(false),
            close_window_on_exit: restored_ssh_profile.is_none(),
            pending_resume_command: RefCell::new(None),
        });
        let callbacks = ffi::KittySessionCallbacks {
            userdata: (&mut *callback_ui as *mut CallbackUi).cast(),
            on_damage: Some(on_damage),
            on_title: Some(on_title),
            on_bell: Some(on_bell),
            on_child_exit: Some(on_child_exit),
            on_notification: None,
            on_user_var: Some(on_user_var),
        };
        let shell_env = CString::new(format!("SHELL={}", account.shell.to_string_lossy())).unwrap();
        let color_env = CString::new("COLORTERM=truecolor").unwrap();
        let environment = [shell_env.as_ptr(), color_env.as_ptr(), ptr::null()];
        let login = CString::new("-il").unwrap();
        let ssh_marker_executable = CString::new("/usr/bin/printf").unwrap();
        let ssh_marker_format = CString::new("%s\\n").unwrap();
        let ssh_marker =
            CString::new("SSH session restored disconnected; use explicit reconnect.").unwrap();
        let argv = if restored_ssh_profile.is_some() {
            [
                ssh_marker_executable.as_ptr(),
                ssh_marker_format.as_ptr(),
                ssh_marker.as_ptr(),
                ptr::null(),
            ]
        } else {
            [
                account.shell.as_ptr(),
                login.as_ptr(),
                ptr::null(),
                ptr::null(),
            ]
        };
        let cwd = path_cstring(restored_cwd.as_deref().unwrap_or(&account.home))
            .map_err(|_| "invalid-cwd".to_owned())?;
        let session = unsafe {
            ffi::kitty_session_create_with_options(
                engine,
                24,
                80,
                argv.as_ptr(),
                cwd.as_ptr(),
                environment.as_ptr(),
                &callbacks,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        if session.is_null() {
            unsafe { ffi::kitty_engine_shutdown(engine) };
            return Err(kitty_stage_error("session-create", &error));
        }
        self.engine = engine;
        self.session = session;
        self.ssh_profile_id = restored_ssh_profile;
        self.resume_command = restored_resume_command;
        self.callback_ui = Some(callback_ui);
        for (surface, cwd) in surface_cwds {
            if surface != self.active_surface_id
                && !self.spawn_restored_surface(
                    surface,
                    &cwd,
                    surface_resume_commands.get(&surface).cloned(),
                    surface_ssh_profiles.get(&surface).copied(),
                )
            {
                diagnostic(
                    "state_restore_surface_failed",
                    &[("surface", surface.to_string())],
                );
            }
        }
        let fd = unsafe { ffi::kitty_session_fd(session) };
        let status_text = format!(
            "Live shell · cell {}×{} px{}",
            self.cell_width,
            self.cell_height,
            self.control_notice
                .as_deref()
                .map_or(String::new(), |notice| format!(" · {notice}"))
        );
        status.set_text(&status_text);
        area.grab_focus();
        diagnostic(
            "terminal_ready",
            &[
                (
                    "pid",
                    unsafe { ffi::kitty_session_child_pid(session) }.to_string(),
                ),
                ("scale", format!("{scale:.2}")),
                ("xdg", "resolved".to_owned()),
                ("backend", area.display().type_().name().to_string()),
            ],
        );
        Ok(fd)
    }

    pub(crate) fn split_layout(&self, area: &GLArea) -> Option<SplitLayout> {
        let (rect, gap, minimum) = split_geometry(area);
        let navigation = self.navigation.as_ref()?;
        if let Some(pane) = self.zoomed_pane
            && navigation.active_tab().pane(pane).is_some()
        {
            return Some(SplitLayout {
                pane_frames: HashMap::from([(pane, rect)]),
                split_frames: HashMap::new(),
                divider_frames: HashMap::new(),
            });
        }
        Some(navigation.active_tab().layout(rect, gap, minimum))
    }

    pub(crate) fn focus_pane_at(&mut self, area: &GLArea, x: f64, y: f64) -> bool {
        let scale = f64::from(area.scale_factor().max(1));
        let layout = match self.split_layout(area) {
            Some(layout) => layout,
            None => return false,
        };
        let Some(pane) = layout
            .pane_frames
            .iter()
            .find_map(|(pane, frame)| rect_contains(*frame, x * scale, y * scale).then_some(*pane))
        else {
            return false;
        };
        let Some(navigation) = self.navigation.as_mut() else {
            return false;
        };
        pane != navigation.active_tab().focused_pane_id() && navigation.focus_pane(pane)
    }

    pub(crate) fn divider_at(&self, area: &GLArea, x: f64, y: f64) -> Option<SplitId> {
        let scale = f64::from(area.scale_factor().max(1));
        let padding = SPLIT_GAP * area.scale_factor().max(1);
        let layout = self.split_layout(area)?;
        layout
            .divider_frames
            .iter()
            .filter(|(_, frame)| {
                rect_contains(
                    PixelRect::new(
                        frame.x - padding,
                        frame.y - padding,
                        frame.width + padding * 2,
                        frame.height + padding * 2,
                    ),
                    x * scale,
                    y * scale,
                )
            })
            .min_by_key(|(split, _)| {
                let frame = layout.split_frames[split];
                i64::from(frame.width) * i64::from(frame.height)
            })
            .map(|(split, _)| *split)
    }

    pub(crate) fn resize_divider(
        &mut self,
        area: &GLArea,
        split_id: SplitId,
        x: f64,
        y: f64,
    ) -> bool {
        let scale = f64::from(area.scale_factor().max(1));
        let (_, gap, minimum) = split_geometry(area);
        let Some(layout) = self.split_layout(area) else {
            return false;
        };
        let Some(split_rect) = layout.split_frames.get(&split_id).copied() else {
            return false;
        };
        let Some(axis) = self
            .navigation
            .as_ref()
            .and_then(|navigation| navigation.active_tab().root().split(split_id))
            .map(|split| split.axis)
        else {
            return false;
        };
        let ratio = match axis {
            SplitAxis::LeftRight => {
                (x * scale - f64::from(split_rect.x)) / f64::from((split_rect.width - gap).max(1))
            }
            SplitAxis::TopBottom => {
                (y * scale - f64::from(split_rect.y)) / f64::from((split_rect.height - gap).max(1))
            }
        };
        let Some(navigation) = self.navigation.as_mut() else {
            return false;
        };
        navigation
            .active_tab_mut()
            .set_split_ratio(split_id, ratio, split_rect, gap, minimum)
    }

    pub(crate) fn render(&mut self, area: &GLArea, status: &Label) {
        if self.engine.is_null() {
            return;
        }
        let factor = area.scale_factor().max(1);
        let width = area.width().max(1) * factor;
        let height = area.height().max(1) * factor;
        let Some(navigation) = self.navigation.as_ref() else {
            return;
        };
        let rect = PixelRect::new(0, 0, width, height);
        let layout = if let Some(pane) = self.zoomed_pane {
            navigation.active_tab().pane(pane).map(|_| SplitLayout {
                pane_frames: HashMap::from([(pane, rect)]),
                split_frames: HashMap::new(),
                divider_frames: HashMap::new(),
            })
        } else {
            Some(
                navigation
                    .active_tab()
                    .layout(rect, SPLIT_GAP * factor, MINIMUM_PANE),
            )
        };
        let Some(layout) = layout else {
            return;
        };
        let visible = navigation
            .runtime_presentations()
            .into_iter()
            .filter(|presentation| presentation.surface_visible)
            .filter(|presentation| {
                self.zoomed_pane
                    .is_none_or(|pane| presentation.location.pane_id == pane)
            })
            .filter_map(|presentation| {
                layout
                    .pane_frames
                    .get(&presentation.location.pane_id)
                    .copied()
                    .map(|frame| (presentation.location.surface_id, frame))
            })
            .collect::<Vec<_>>();
        let mut region_surfaces = Vec::with_capacity(visible.len());
        let mut regions = visible
            .iter()
            .filter_map(|(surface, frame)| {
                let session = self.sessions.get(surface)?;
                if session.session.is_null() {
                    return None;
                }
                region_surfaces.push(*surface);
                Some(TerminalRegion {
                    session: session.session,
                    x: frame.x,
                    y: frame.y,
                    width: frame.width,
                    height: frame.height,
                    previous_width: session.framebuffer_width,
                    previous_height: session.framebuffer_height,
                    viewport_changed: false,
                })
            })
            .collect::<Vec<_>>();
        if regions.is_empty() {
            return;
        }
        let mut cell_width = self.cell_width;
        let mut cell_height = self.cell_height;
        let mut error = [0 as c_char; 512];
        let ok = unsafe {
            kitmux_terminal_render_regions(
                self.engine,
                f64::from(factor),
                regions.as_mut_ptr(),
                regions.len(),
                &mut cell_width,
                &mut cell_height,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        if !ok {
            status.set_text("Terminal rendering failed");
            diagnostic("render_failed", &[]);
            return;
        }
        self.cell_width = cell_width;
        self.cell_height = cell_height;
        let mut viewport_changed = false;
        for (surface, region) in region_surfaces.iter().zip(&regions) {
            if let Some(session) = self.sessions.get_mut(surface) {
                session.framebuffer_width = region.width;
                session.framebuffer_height = region.height;
                viewport_changed |= region.viewport_changed;
            }
        }
        if viewport_changed {
            diagnostic(
                "viewport",
                &[
                    ("width", width.to_string()),
                    ("height", height.to_string()),
                    ("factor", factor.to_string()),
                    ("panes", regions.len().to_string()),
                ],
            );
        }
    }

    pub(crate) fn route_key(&mut self, input: &KitmuxGdkKeyInput, committed_text: Option<&str>) {
        if self.session.is_null() {
            return;
        }
        let mut translated: KitmuxKeyTranslation = unsafe { std::mem::zeroed() };
        let committed = committed_text.and_then(|text| CString::new(text).ok());
        if !unsafe {
            ffi::kitmux_translate_gdk_key(
                input,
                committed.as_ref().map_or(ptr::null(), |text| text.as_ptr()),
                &mut translated,
            )
        } {
            return;
        }
        let mut encoded = [0 as c_char; 256];
        let count = unsafe {
            ffi::kitty_session_encode_key(
                self.session,
                &translated.event,
                encoded.as_mut_ptr(),
                encoded.len(),
            )
        };
        if count > 0 {
            unsafe { ffi::kitty_session_write(self.session, encoded.as_ptr().cast(), count) };
        }
    }

    pub(crate) fn shortcut(
        &self,
        keyval: gdk::Key,
        state: gdk::ModifierType,
    ) -> Option<ShortcutAction> {
        let mut key = keyval.to_unicode()?.to_ascii_lowercase();
        let mut shift = state.contains(gdk::ModifierType::SHIFT_MASK);
        if (key == '=' || key == '+') && shift {
            key = '+';
            shift = false;
        }
        if shift {
            key = match key {
                '{' => '[',
                '}' => ']',
                _ => key,
            };
        }
        let chord = ShortcutChord {
            key,
            control: state.contains(gdk::ModifierType::CONTROL_MASK),
            shift,
            alt: state.contains(gdk::ModifierType::ALT_MASK),
            super_key: state.contains(gdk::ModifierType::SUPER_MASK),
        };
        namespaced_number_target(chord)
            .map(ShortcutAction::Select)
            .or_else(|| self.shortcuts.resolve(chord))
    }

    pub(crate) fn spawn_surface(&mut self, surface_id: SurfaceId) -> bool {
        let cwd = self.account_home.clone();
        self.spawn_surface_at(surface_id, &cwd)
    }

    pub(crate) fn spawn_surface_at(&mut self, surface_id: SurfaceId, cwd: &Path) -> bool {
        let account = account();
        let login = OsString::from("-il");
        self.spawn_surface_command(
            surface_id,
            cwd,
            vec![
                OsString::from(account.shell.to_string_lossy().into_owned()),
                login,
            ],
            None,
        )
    }

    pub(crate) fn spawn_restored_surface(
        &mut self,
        surface_id: SurfaceId,
        cwd: &Path,
        resume_command: Option<String>,
        ssh_profile_id: Option<Uuid>,
    ) -> bool {
        let argv = ssh_profile_id.map_or_else(
            || {
                let account = account();
                vec![
                    OsString::from(account.shell.to_string_lossy().into_owned()),
                    OsString::from("-il"),
                ]
            },
            disconnected_ssh_argv,
        );
        if !self.spawn_surface_command(surface_id, cwd, argv, ssh_profile_id) {
            return false;
        }
        if let Some(session) = self.sessions.get_mut(&surface_id) {
            session.resume_command = resume_command;
        }
        true
    }

    pub(crate) fn spawn_surface_command(
        &mut self,
        surface_id: SurfaceId,
        cwd: &Path,
        argv: Vec<OsString>,
        ssh_profile_id: Option<Uuid>,
    ) -> bool {
        if self.engine.is_null() || self.sessions.contains_key(&surface_id) {
            return false;
        }
        let Some(session) = self.create_session_state(surface_id, cwd, argv, ssh_profile_id) else {
            return false;
        };
        let pid = unsafe { ffi::kitty_session_child_pid(session.session) };
        self.sessions.insert(surface_id, session);
        diagnostic(
            if ssh_profile_id.is_some() {
                "ssh_surface_created"
            } else {
                "terminal_surface_created"
            },
            &[
                ("surface", surface_id.to_string()),
                ("pid", pid.to_string()),
            ],
        );
        true
    }

    pub(crate) fn create_session_state(
        &self,
        surface_id: SurfaceId,
        cwd: &Path,
        argv: Vec<OsString>,
        ssh_profile_id: Option<Uuid>,
    ) -> Option<SessionState> {
        let ui = self.navigation_ui.as_ref()?;
        let (Some(window), Some(area), Some(status)) =
            (ui.window.upgrade(), ui.area.upgrade(), ui.status.upgrade())
        else {
            return None;
        };
        let account = account();
        let arguments = argv
            .iter()
            .map(|value| CString::new(value.as_os_str().as_bytes()).ok())
            .collect::<Option<Vec<_>>>()?;
        let mut argument_pointers = arguments
            .iter()
            .map(|value| value.as_ptr())
            .collect::<Vec<_>>();
        argument_pointers.push(ptr::null());
        let environment = session_environment(&account, ssh_profile_id.is_some())
            .into_iter()
            .map(|value| CString::new(value.as_os_str().as_bytes()).ok())
            .collect::<Option<Vec<_>>>()?;
        let mut environment_pointers = environment
            .iter()
            .map(|value| value.as_ptr())
            .collect::<Vec<_>>();
        environment_pointers.push(ptr::null());
        let cwd = if valid_restored_cwd(cwd) {
            cwd
        } else {
            &account.home
        };
        let cwd_c = path_cstring(cwd).ok()?;
        let mut callback_ui = Box::new(CallbackUi {
            window: window.downgrade(),
            area: area.downgrade(),
            status: status.downgrade(),
            visible: Cell::new(surface_id == self.active_surface_id),
            disconnected: Cell::new(false),
            close_window_on_exit: ssh_profile_id.is_none(),
            pending_resume_command: RefCell::new(None),
        });
        let callbacks = ffi::KittySessionCallbacks {
            userdata: (&mut *callback_ui as *mut CallbackUi).cast(),
            on_damage: Some(on_damage),
            on_title: Some(on_title),
            on_bell: Some(on_bell),
            on_child_exit: Some(on_child_exit),
            on_notification: None,
            on_user_var: Some(on_user_var),
        };
        let mut error = [0 as c_char; 1024];
        let session = unsafe {
            ffi::kitty_session_create_with_options(
                self.engine,
                24,
                80,
                argument_pointers.as_ptr(),
                cwd_c.as_ptr(),
                environment_pointers.as_ptr(),
                &callbacks,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        if session.is_null() {
            diagnostic(
                if ssh_profile_id.is_some() {
                    "ssh_surface_failed"
                } else {
                    "terminal_surface_failed"
                },
                &[("reason", "session-create".to_owned())],
            );
            return None;
        }
        Some(SessionState {
            session,
            callback_ui: Some(callback_ui),
            last_cwd: Some(cwd.to_owned()),
            ssh_profile_id,
            ..SessionState::default()
        })
    }

    pub(crate) fn replace_ssh_surface(
        &mut self,
        surface_id: SurfaceId,
        profile: &SshProfile,
        executable: &Path,
    ) -> bool {
        let Ok(argv) = SshResolution::argv(executable, profile) else {
            return false;
        };
        let cwd = self
            .sessions
            .get(&surface_id)
            .and_then(|session| session.last_cwd.clone())
            .unwrap_or_else(|| self.account_home.clone());
        let Some(new_session) = self.create_session_state(surface_id, &cwd, argv, Some(profile.id))
        else {
            return false;
        };
        if let Some(mut old_session) = self.sessions.remove(&surface_id) {
            if old_session.pty_source != 0 {
                unsafe { g_source_remove(old_session.pty_source) };
            }
            if !old_session.session.is_null() {
                unsafe { ffi::kitty_session_close(old_session.session) };
                old_session.session = ptr::null_mut();
            }
        }
        self.sessions.insert(surface_id, new_session);
        diagnostic("ssh_reconnected", &[("surface", surface_id.to_string())]);
        true
    }

    pub(crate) fn create_ssh_tab(&mut self, profile: &SshProfile, executable: &Path) -> bool {
        let Ok(argv) = SshResolution::argv(executable, profile) else {
            return false;
        };
        let (tab, surface) = pending_tab();
        if !self.spawn_surface_command(surface, &self.account_home.clone(), argv, Some(profile.id))
        {
            return false;
        }
        self.navigation
            .as_mut()
            .and_then(|navigation| {
                navigation
                    .active_workspace_mut()
                    .active_group_mut()
                    .append_tab(tab)
                    .ok()
            })
            .is_some()
    }

    pub(crate) fn navigation_action(&mut self, command: CommandId) -> NavigationEffect {
        if self.navigation.is_none() {
            return NavigationEffect::Rejected;
        }
        match command {
            CommandId::WorkspaceNew => {
                self.created_workspaces += 1;
                let (mut workspace, surface) = pending_workspace();
                workspace.rename(&format!("Workspace {}", self.created_workspaces));
                if !self.spawn_surface(surface) {
                    return NavigationEffect::Rejected;
                }
                let Some(navigation) = self.navigation.as_mut() else {
                    return NavigationEffect::Rejected;
                };
                navigation
                    .append_workspace(workspace)
                    .map_or(NavigationEffect::Rejected, |_| NavigationEffect::Changed)
            }
            CommandId::GroupNew => {
                self.created_groups += 1;
                let (mut group, surface) = pending_group();
                group.rename(&format!("Group {}", self.created_groups));
                if !self.spawn_surface(surface) {
                    return NavigationEffect::Rejected;
                }
                let Some(navigation) = self.navigation.as_mut() else {
                    return NavigationEffect::Rejected;
                };
                navigation
                    .active_workspace_mut()
                    .append_group(group)
                    .map_or(NavigationEffect::Rejected, |_| NavigationEffect::Changed)
            }
            CommandId::TerminalNewTab => {
                let (tab, surface) = pending_tab();
                if !self.spawn_surface(surface) {
                    return NavigationEffect::Rejected;
                }
                let Some(navigation) = self.navigation.as_mut() else {
                    return NavigationEffect::Rejected;
                };
                navigation
                    .active_workspace_mut()
                    .active_group_mut()
                    .append_tab(tab)
                    .map_or(NavigationEffect::Rejected, |_| NavigationEffect::Changed)
            }
            CommandId::PaneSplitRight | CommandId::PaneSplitDown => {
                let pane = PaneId::new();
                let surface = SurfaceId::new();
                if !self.spawn_surface(surface) {
                    return NavigationEffect::Rejected;
                }
                let Some(navigation) = self.navigation.as_mut() else {
                    return NavigationEffect::Rejected;
                };
                let tab = navigation.active_tab_mut();
                let focused = tab.focused_pane_id();
                let axis = if command == CommandId::PaneSplitRight {
                    SplitAxis::LeftRight
                } else {
                    SplitAxis::TopBottom
                };
                tab.split_pane(focused, axis, pending_pane(pane, surface))
                    .map_or(NavigationEffect::Rejected, |_| NavigationEffect::Changed)
            }
            _ => {
                let geometry = self
                    .navigation_ui
                    .as_ref()
                    .and_then(|ui| ui.area.upgrade())
                    .and_then(|area| {
                        let (rect, gap, minimum) = split_geometry(&area);
                        let navigation = self.navigation.as_ref()?;
                        Some((
                            rect,
                            gap,
                            minimum,
                            navigation.active_tab().layout(rect, gap, minimum),
                        ))
                    });
                let Some(navigation) = self.navigation.as_mut() else {
                    return NavigationEffect::Rejected;
                };
                match command {
                    CommandId::TerminalNextTab => changed(
                        navigation
                            .active_workspace_mut()
                            .active_group_mut()
                            .cycle_tab(1),
                    ),
                    CommandId::TerminalPreviousTab => changed(
                        navigation
                            .active_workspace_mut()
                            .active_group_mut()
                            .cycle_tab(-1),
                    ),
                    CommandId::GroupNext => {
                        changed(navigation.active_workspace_mut().cycle_group(1))
                    }
                    CommandId::GroupPrevious => {
                        changed(navigation.active_workspace_mut().cycle_group(-1))
                    }
                    CommandId::TabClose => {
                        let group = navigation.active_workspace_mut().active_group_mut();
                        (group.tabs().len() > 1)
                            .then(|| group.close_tab(group.active_tab_index()))
                            .flatten()
                            .map_or(NavigationEffect::Rejected, |_| NavigationEffect::Changed)
                    }
                    CommandId::PaneClose => {
                        let pane = navigation.active_tab().focused_pane_id();
                        match navigation.close_pane(pane) {
                            Some(CloseOutcome::Removed(_)) => NavigationEffect::Changed,
                            Some(CloseOutcome::HostCloseRequired(_)) => {
                                NavigationEffect::CloseWindow
                            }
                            None => NavigationEffect::Rejected,
                        }
                    }
                    CommandId::GroupClose => {
                        let index = navigation.active_workspace().active_group_index();
                        changed(
                            navigation
                                .active_workspace_mut()
                                .close_group(index)
                                .is_some(),
                        )
                    }
                    CommandId::WorkspaceClose => {
                        let index = navigation.active_workspace_index();
                        changed(navigation.close_workspace(index).is_some())
                    }
                    CommandId::WorkspaceRename => NavigationEffect::Rename(
                        RenameTarget::Workspace(navigation.active_workspace().id()),
                    ),
                    CommandId::GroupRename => NavigationEffect::Rename(RenameTarget::Group(
                        navigation.active_workspace().active_group().id(),
                    )),
                    CommandId::TerminalRenameTab => {
                        NavigationEffect::Rename(RenameTarget::Tab(navigation.active_tab().id()))
                    }
                    CommandId::PaneFocusNext => changed(navigation.active_tab_mut().cycle_focus(1)),
                    CommandId::PaneFocusPrevious => {
                        changed(navigation.active_tab_mut().cycle_focus(-1))
                    }
                    CommandId::PaneFocusLeft
                    | CommandId::PaneFocusRight
                    | CommandId::PaneFocusUp
                    | CommandId::PaneFocusDown => {
                        let direction = match command {
                            CommandId::PaneFocusLeft => Direction::Left,
                            CommandId::PaneFocusRight => Direction::Right,
                            CommandId::PaneFocusUp => Direction::Up,
                            _ => Direction::Down,
                        };
                        changed(geometry.is_some_and(|(_, _, _, layout)| {
                            navigation.active_tab_mut().move_focus(direction, &layout)
                        }))
                    }
                    CommandId::PaneResizeLeft
                    | CommandId::PaneResizeRight
                    | CommandId::PaneResizeUp
                    | CommandId::PaneResizeDown => {
                        let direction = match command {
                            CommandId::PaneResizeLeft => Direction::Left,
                            CommandId::PaneResizeRight => Direction::Right,
                            CommandId::PaneResizeUp => Direction::Up,
                            _ => Direction::Down,
                        };
                        let resized = geometry.is_some_and(|(rect, gap, minimum, _)| {
                            navigation
                                .active_tab_mut()
                                .resize_focused(direction, rect, gap, minimum, 0.05)
                        });
                        if resized {
                            let direction = match direction {
                                Direction::Left => "left",
                                Direction::Right => "right",
                                Direction::Up => "up",
                                Direction::Down => "down",
                            };
                            diagnostic("pane_resized", &[("direction", direction.to_owned())]);
                        }
                        changed(resized)
                    }
                    _ => NavigationEffect::Rejected,
                }
            }
        }
    }

    pub(crate) fn select_navigation_target(
        &mut self,
        target: NavigationTarget,
    ) -> NavigationEffect {
        let Some(navigation) = self.navigation.as_mut() else {
            return NavigationEffect::Rejected;
        };
        match target {
            NavigationTarget::Workspace(index) => changed(navigation.select_workspace(index)),
            NavigationTarget::TerminalTab(index) => changed(
                navigation
                    .active_workspace_mut()
                    .active_group_mut()
                    .select_tab(index),
            ),
        }
    }

    pub(crate) fn rename_navigation(&mut self, target: RenameTarget, name: &str) -> bool {
        let Some(navigation) = self.navigation.as_mut() else {
            return false;
        };
        match target {
            RenameTarget::Workspace(id) => navigation.rename_workspace(id, name),
            RenameTarget::Group(id) => navigation.rename_group(id, name),
            RenameTarget::Tab(id) => navigation.rename_tab(id, Some(name)),
        }
    }

    pub(crate) fn move_active_workspace(&mut self, direction: isize) -> bool {
        let Some(navigation) = self.navigation.as_mut() else {
            return false;
        };
        let index = navigation.active_workspace_index();
        let target = index.saturating_add_signed(direction);
        let id = navigation.active_workspace().id();
        navigation.move_workspace(id, target)
    }

    pub(crate) fn move_active_tab(&mut self, direction: isize) -> bool {
        let Some(navigation) = self.navigation.as_mut() else {
            return false;
        };
        let group = navigation.active_workspace_mut().active_group_mut();
        let index = group.active_tab_index();
        let target = index.saturating_add_signed(direction);
        let id = group.active_tab().id();
        group.move_tab(id, target)
    }

    pub(crate) fn clear_selection(&mut self) {
        if !self.session.is_null() {
            unsafe { ffi::kitty_session_selection_clear(self.session) };
        }
        self.selection_active = false;
    }

    pub(crate) fn selection_text(&self) -> Option<String> {
        if self.session.is_null() {
            return None;
        }
        owned_c_string(unsafe { ffi::kitty_session_selection_text(self.session) })
            .filter(|text| !text.is_empty())
    }

    pub(crate) fn select_all(&mut self, area: &GLArea) {
        if self.session.is_null() || self.cell_width <= 0 || self.cell_height <= 0 {
            return;
        }
        let columns = (self.framebuffer_width / self.cell_width).max(1) as u32;
        let rows = (self.framebuffer_height / self.cell_height).max(1) as u32;
        // ponytail: select the current terminal grid; libkitty exposes no
        // history-wide select-all primitive, so a future engine API can widen it.
        unsafe {
            ffi::kitty_session_selection_start(self.session, 0, 0, true, 0);
            ffi::kitty_session_selection_update(self.session, columns - 1, rows - 1, false, true);
        }
        self.selection_active = true;
        area.queue_render();
        diagnostic(
            "selection_all",
            &[("columns", columns.to_string()), ("rows", rows.to_string())],
        );
    }

    pub(crate) fn toggle_zoom(&mut self, area: &GLArea) {
        let Some(pane) = self
            .navigation
            .as_ref()
            .map(|navigation| navigation.active_tab().focused_pane_id())
        else {
            return;
        };
        self.zoomed_pane = (self.zoomed_pane != Some(pane)).then_some(pane);
        area.queue_render();
        diagnostic(
            "pane_zoom",
            &[("enabled", self.zoomed_pane.is_some().to_string())],
        );
    }

    pub(crate) fn toggle_fullscreen(&mut self, window: &ApplicationWindow) {
        self.fullscreen = !self.fullscreen;
        window.set_fullscreened(self.fullscreen);
        diagnostic("fullscreen", &[("enabled", self.fullscreen.to_string())]);
    }

    pub(crate) fn paste(&mut self, text: &str) {
        if self.session.is_null() || text.is_empty() {
            return;
        }
        self.clear_selection();
        unsafe { ffi::kitty_session_paste(self.session, text.as_ptr(), text.len()) };
        diagnostic("paste", &[("bytes", text.len().to_string())]);
    }

    pub(crate) fn set_font_size(&mut self, area: &GLArea, points: f64) {
        if self.engine.is_null() || !points.is_finite() {
            return;
        }
        let upper = (self.default_font_size * 10.0).max(4.0);
        let points = points.clamp(4.0, upper);
        area.make_current();
        let mut error = [0 as c_char; 512];
        if unsafe {
            ffi::kitty_render_set_font_size(
                self.engine,
                points,
                &mut self.cell_width,
                &mut self.cell_height,
                error.as_mut_ptr(),
                error.len(),
            )
        } {
            self.framebuffer_width = 0;
            self.framebuffer_height = 0;
            area.queue_render();
            diagnostic("font_size", &[("points", format!("{points:.2}"))]);
        } else {
            area.error_bell();
            diagnostic("font_size_failed", &[]);
        }
    }

    pub(crate) fn change_font_size(&mut self, area: &GLArea, delta: f64) {
        if self.engine.is_null() {
            return;
        }
        let current = unsafe { ffi::kitty_render_font_size(self.engine) };
        self.set_font_size(area, current + delta);
    }

    pub(crate) fn search(&mut self, query: &str) -> Result<usize, String> {
        if self.session.is_null() || query.is_empty() {
            if !self.session.is_null() {
                unsafe { ffi::kitty_session_search_clear(self.session) };
                diagnostic("search_cleared", &[]);
            }
            return Ok(0);
        }
        let mut count = 0;
        let mut error = [0 as c_char; 512];
        if unsafe {
            ffi::kitty_session_search_set_options(
                self.session,
                query.as_ptr().cast(),
                query.len(),
                false,
                false,
                &mut count,
                error.as_mut_ptr(),
                error.len(),
            )
        } {
            if let Some(ui) = &self.callback_ui
                && let Some(area) = ui.area.upgrade()
            {
                area.queue_render();
            }
            diagnostic("search_updated", &[("matches", count.to_string())]);
            Ok(count)
        } else {
            Err(c_buffer(&error).unwrap_or_else(|| "invalid search".to_owned()))
        }
    }

    pub(crate) fn navigate_search(&mut self, backwards: bool) -> bool {
        if self.session.is_null() {
            return false;
        }
        let found = unsafe { ffi::kitty_session_search_next(self.session, backwards) };
        if let Some(ui) = &self.callback_ui
            && let Some(area) = ui.area.upgrade()
        {
            area.queue_render();
        }
        diagnostic(
            "search_navigated",
            &[
                ("backwards", backwards.to_string()),
                ("found", found.to_string()),
            ],
        );
        found
    }

    pub(crate) fn cell_at(
        &self,
        area: &GLArea,
        x: f64,
        y: f64,
    ) -> Option<kitmux_model::TerminalCellCoordinate> {
        let frame = self
            .split_layout(area)?
            .pane_frames
            .get(&self.navigation.as_ref()?.active_tab().focused_pane_id())
            .copied()?;
        terminal_cell_scaled(
            x,
            y,
            f64::from(area.scale_factor()),
            frame,
            self.cell_width,
            self.cell_height,
        )
    }

    pub(crate) fn send_mouse(
        &mut self,
        area: &GLArea,
        x: f64,
        y: f64,
        button: c_int,
        action: c_int,
        state: gdk::ModifierType,
    ) -> bool {
        if self.session.is_null() {
            return false;
        }
        let Some(cell) = self.cell_at(area, x, y) else {
            return false;
        };
        let mut mods = 0;
        if state.contains(gdk::ModifierType::ALT_MASK) {
            mods |= 0x2;
        }
        if state.contains(gdk::ModifierType::CONTROL_MASK) {
            mods |= 0x4;
        }
        let forwarded = unsafe {
            ffi::kitty_session_mouse_event(
                self.session,
                cell.column,
                cell.row,
                button,
                action,
                mods,
                cell.pixel_x,
                cell.pixel_y,
            ) != 0
        };
        if forwarded && env::var_os("KITMUX_INTERACTION_DIAGNOSTICS").is_some() {
            diagnostic(
                "mouse_forwarded",
                &[
                    ("button", button.to_string()),
                    ("action", action.to_string()),
                    ("column", cell.column.to_string()),
                    ("row", cell.row.to_string()),
                ],
            );
        }
        forwarded
    }

    pub(crate) fn start_selection(&mut self, area: &GLArea, x: f64, y: f64, press_count: c_int) {
        let Some(cell) = self.cell_at(area, x, y) else {
            return;
        };
        let mode = match press_count {
            2 => 1,
            3.. => 2,
            _ => 0,
        };
        unsafe {
            ffi::kitty_session_selection_start(
                self.session,
                cell.column,
                cell.row,
                cell.in_left_half,
                mode,
            )
        };
        self.selection_active = true;
        area.queue_render();
    }

    pub(crate) fn update_selection(&mut self, area: &GLArea, x: f64, y: f64, ended: bool) {
        if !self.selection_active {
            return;
        }
        let Some(cell) = self.cell_at(area, x, y) else {
            return;
        };
        unsafe {
            ffi::kitty_session_selection_update(
                self.session,
                cell.column,
                cell.row,
                cell.in_left_half,
                ended,
            )
        };
        if ended {
            self.selection_active = false;
        }
        area.queue_render();
    }

    pub(crate) fn url_at(&self, area: &GLArea, x: f64, y: f64) -> Option<String> {
        if self.session.is_null() {
            return None;
        }
        let cell = self.cell_at(area, x, y)?;
        let text = owned_c_string(unsafe { ffi::kitty_session_text(self.session) })?;
        let rows: Vec<String> = text.lines().map(str::to_owned).collect();
        let mut wraps = vec![0_u8; rows.len()];
        let count =
            unsafe { ffi::kitty_session_line_wraps(self.session, wraps.as_mut_ptr(), wraps.len()) };
        wraps.truncate(count);
        let wraps: Vec<bool> = wraps.into_iter().map(|value| value != 0).collect();
        let columns = (self.framebuffer_width / self.cell_width.max(1)).max(2) as usize;
        detected_url(
            &rows,
            cell.row as usize,
            cell.column as usize,
            columns,
            Some(&wraps),
        )
        .map(|found| found.url)
    }

    pub(crate) fn foreground_surfaces(&self, scope: Option<ForegroundScope>) -> Vec<SurfaceId> {
        let Some(navigation) = self.navigation.as_ref() else {
            return Vec::new();
        };
        let active_workspace = navigation.active_workspace().id();
        let active_group = navigation.active_workspace().active_group().id();
        let active_tab = navigation.active_tab().id();
        let active_pane = navigation.active_tab().focused_pane_id();
        navigation
            .runtime_presentations()
            .into_iter()
            .filter(|presentation| match scope {
                Some(ForegroundScope::Pane) => presentation.location.pane_id == active_pane,
                Some(ForegroundScope::Tab) => presentation.location.tab_id == active_tab,
                Some(ForegroundScope::Group) => presentation.location.group_id == active_group,
                Some(ForegroundScope::Workspace) => {
                    presentation.location.workspace_id == active_workspace
                }
                None => true,
            })
            .filter_map(|presentation| {
                let surface = presentation.location.surface_id;
                self.sessions.get(&surface).and_then(|session| {
                    (!session.session.is_null()
                        && unsafe { ffi::kitty_session_has_foreground_process(session.session) })
                    .then_some(surface)
                })
            })
            .collect()
    }

    pub(crate) fn im_commit(&mut self, text: &str) {
        if text.is_empty() || self.session.is_null() {
            return;
        }
        let single_scalar = text.chars().count() == 1;
        if self.filtering
            && !self.filtering_committed
            && !self.filtering_had_preedit
            && single_scalar
            && text.len() < 32
        {
            self.filtering_committed = true;
            self.filtering_encoded = true;
            let input = self.filtering_input;
            self.route_key(&input, Some(text));
            return;
        }
        self.filtering_committed = true;
        unsafe { ffi::kitty_session_write(self.session, text.as_ptr(), text.len()) };
    }

    pub(crate) fn refresh_cwd(&mut self) {
        if self.session.is_null() {
            return;
        }
        let value = unsafe { ffi::kitty_session_reported_cwd(self.session) };
        if value.is_null() {
            return;
        }
        let cwd = unsafe { CStr::from_ptr(value) };
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(cwd.to_bytes()));
        unsafe { libc::free(value.cast()) };
        if path.is_absolute() && self.last_cwd.as_ref() != Some(&path) {
            self.last_cwd = Some(path);
            diagnostic("cwd_updated", &[("valid", "true".to_owned())]);
        }
    }

    pub(crate) fn poll_settings(&mut self) {
        let Some(persistence) = self.persistence.as_mut() else {
            return;
        };
        let Some(watcher) = persistence.settings_watcher.as_mut() else {
            return;
        };
        if !matches!(watcher.poll(), Ok(Some(_))) {
            return;
        }
        let Some(document) = reload_settings(&persistence.settings_path) else {
            return;
        };
        if document == persistence.settings {
            return;
        }
        self.apply_settings(document);
    }

    pub(crate) fn apply_settings(&mut self, document: SettingsDocument) {
        let resolved = document.resolved();
        let threshold =
            usize::try_from(resolved.paste_confirmation_threshold_bytes).unwrap_or(usize::MAX);
        let wheel_scroll_lines = document.wheel_scroll_lines();
        let confirm = resolved.confirm_close_with_running_process;
        if let Some(sidebar) = self
            .navigation_ui
            .as_ref()
            .and_then(|ui| ui.sidebar_shell.upgrade())
        {
            sidebar.set_visible(resolved.sidebar_visible_on_launch);
            sidebar.set_width_request(resolved.sidebar_width_points as i32);
        }
        self.shortcuts = ShortcutMap::linux_from_settings(&document);
        let menu_bar_visible = resolved.menu_bar_visible_on_launch;
        if let Some(window) = self
            .navigation_ui
            .as_ref()
            .and_then(|ui| ui.window.upgrade())
        {
            window.set_show_menubar(menu_bar_visible);
        }
        if let Some(menu_bar) = self
            .navigation_ui
            .as_ref()
            .and_then(|ui| ui.menu_bar.upgrade())
        {
            menu_bar.set_visible(menu_bar_visible);
        }
        if let Some(app) = self.navigation_ui.as_ref().and_then(|ui| ui.app.upgrade()) {
            set_menu_accelerators(&app, &self.shortcuts);
        }
        if let Some(persistence) = self.persistence.as_mut() {
            persistence.settings = document;
        }
        self.paste_confirmation_threshold = threshold;
        self.wheel_scroll_lines = wheel_scroll_lines;
        self.confirm_close_with_running_process = confirm;
        diagnostic(
            "settings_reloaded",
            &[
                ("paste_threshold", threshold.to_string()),
                ("wheel_lines", wheel_scroll_lines.to_string()),
                ("confirm_close", confirm.to_string()),
            ],
        );
    }

    pub(crate) fn snapshot(&self) -> AppSnapshot {
        let font_size = (!self.engine.is_null())
            .then(|| unsafe { ffi::kitty_render_font_size(self.engine) })
            .filter(|points| points.is_finite());
        let workspaces = self
            .navigation
            .as_ref()
            .map(|navigation| {
                navigation
                    .workspaces()
                    .iter()
                    .map(|workspace| WorkspaceSnapshot {
                        id: Some(workspace.id()),
                        name: workspace.name().to_owned(),
                        active_tab_group_index: workspace.active_group_index() as i64,
                        created_group_count: self.created_groups as i64,
                        tab_groups: workspace
                            .groups()
                            .iter()
                            .map(|group| TabGroupSnapshot {
                                name: group.name().to_owned(),
                                active_terminal_tab_index: group.active_tab_index() as i64,
                                terminal_tabs: group
                                    .tabs()
                                    .iter()
                                    .map(|tab| {
                                        let pane_details = tab
                                            .pane_ids()
                                            .into_iter()
                                            .filter_map(|pane_id| {
                                                let pane = tab.pane(pane_id)?;
                                                if let Some(profile_id) =
                                                    pane.surfaces().iter().find_map(|surface| {
                                                        self.sessions.get(&surface.id()).and_then(
                                                            |session| session.ssh_profile_id,
                                                        )
                                                    })
                                                {
                                                    return Some((
                                                        pane_id.to_string(),
                                                        PaneDetail {
                                                            ssh_profile_id: Some(profile_id),
                                                            ..PaneDetail::default()
                                                        },
                                                    ));
                                                }
                                                let surfaces = pane
                                                    .surfaces()
                                                    .iter()
                                                    .filter(|surface| {
                                                        surface.kind()
                                                            == kitmux_model::RuntimeKind::Terminal
                                                    })
                                                    .map(|surface| {
                                                        let cwd = self
                                                            .sessions
                                                            .get(&surface.id())
                                                            .and_then(|session| {
                                                                session.last_cwd.as_ref()
                                                            })
                                                            .filter(|path| valid_restored_cwd(path))
                                                            .unwrap_or(&self.account_home)
                                                            .to_string_lossy()
                                                            .into_owned();
                                                        PaneSurfaceDetail {
                                                            id: surface.id().as_uuid(),
                                                            cwd: Some(cwd),
                                                            resume_command: self
                                                                .sessions
                                                                .get(&surface.id())
                                                                .and_then(|session| {
                                                                    session.resume_command.clone()
                                                                }),
                                                            kind: PaneContentKind::Terminal,
                                                            url: None,
                                                        }
                                                    })
                                                    .collect::<Vec<_>>();
                                                (!surfaces.is_empty()).then(|| {
                                                    (
                                                        pane_id.to_string(),
                                                        PaneDetail {
                                                            surfaces: Some(surfaces),
                                                            active_surface_index: Some(
                                                                pane.active_surface_index() as i64,
                                                            ),
                                                            ..PaneDetail::default()
                                                        },
                                                    )
                                                })
                                            })
                                            .collect::<BTreeMap<_, _>>();
                                        TerminalTabSnapshot {
                                            focused_pane_id: tab.focused_pane_id(),
                                            root: tab.root().clone(),
                                            custom_title: tab.custom_title().map(str::to_owned),
                                            pane_details: (!pane_details.is_empty())
                                                .then_some(pane_details),
                                        }
                                    })
                                    .collect(),
                            })
                            .collect(),
                        color_index: None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        AppSnapshot {
            version: SNAPSHOT_VERSION,
            active_workspace_index: self
                .navigation
                .as_ref()
                .map_or(0, |navigation| navigation.active_workspace_index() as i64),
            created_workspace_count: self.created_workspaces as i64,
            workspaces,
            font_size,
        }
    }

    pub(crate) fn persist_state_now(&self) -> Result<(), String> {
        let Some(persistence) = self.persistence.as_ref() else {
            return Err("persistence unavailable".to_owned());
        };
        if !persistence.state_may_write {
            return Err("state input was not safely writable".to_owned());
        }
        save_state(&persistence.state_path, self.snapshot())
    }

    pub(crate) fn resume_current_state(
        &self,
        surface_id: SurfaceId,
    ) -> Option<ResumeCommandCurrentState> {
        let pane_id = self
            .navigation
            .as_ref()?
            .runtime_presentations()
            .into_iter()
            .find(|presentation| presentation.location.surface_id == surface_id)?
            .location
            .pane_id;
        let session = self.sessions.get(&surface_id)?;
        let cwd = session
            .last_cwd
            .as_deref()
            .filter(|path| valid_restored_cwd(path))
            .unwrap_or(&self.account_home)
            .to_string_lossy()
            .into_owned();
        Some(ResumeCommandCurrentState {
            pane_id,
            surface_id,
            command: session.resume_command.clone(),
            cwd: Some(cwd),
            is_eligible: !session.session.is_null()
                && unsafe { ffi::kitty_session_child_alive(session.session) }
                && session.ssh_profile_id.is_none(),
        })
    }

    pub(crate) fn run_resume_commands(&mut self, policy: ResumeCommandSelectionPolicy) {
        for surface_id in policy.selected_row_ids() {
            let Some(displayed) = policy
                .displayed_rows()
                .iter()
                .find(|row| row.surface_id == surface_id)
                .cloned()
            else {
                continue;
            };
            let Some(current) = self.resume_current_state(surface_id) else {
                continue;
            };
            if !policy
                .executable_rows(std::slice::from_ref(&current))
                .contains(&displayed)
            {
                diagnostic(
                    "resume_command_skipped",
                    &[("reason", "identity-changed".to_owned())],
                );
                continue;
            }
            let Some(session) = self.sessions.get(&surface_id) else {
                continue;
            };
            if session.session.is_null()
                || session.ssh_profile_id.is_some()
                || !unsafe { ffi::kitty_session_child_alive(session.session) }
            {
                continue;
            }
            let bytes = displayed.command.as_bytes();
            unsafe {
                ffi::kitty_session_write(session.session, bytes.as_ptr(), bytes.len());
                ffi::kitty_session_write(session.session, b"\r".as_ptr(), 1);
            }
            diagnostic(
                "resume_command_executed",
                &[("pane", displayed.pane_id.to_string())],
            );
        }
    }

    pub(crate) fn shutdown(&mut self, area: &GLArea) {
        if self.engine.is_null()
            && self
                .sessions
                .values()
                .all(|session| session.session.is_null())
        {
            return;
        }
        CONTROL_WAKE.with(|wake| {
            wake.borrow_mut().take();
        });
        self.control_server.take();
        if let Some(source) = self.settings_source.take() {
            source.remove();
        }
        if let Some(persistence) = self.persistence.take() {
            if persistence.state_may_write {
                match save_state(&persistence.state_path, self.snapshot()) {
                    Ok(()) => diagnostic("state_saved", &[]),
                    Err(_) => diagnostic("state_save_failed", &[]),
                }
            } else {
                diagnostic(
                    "state_save_skipped",
                    &[("reason", "unsafe-input".to_owned())],
                );
            }
        }
        if self
            .sessions
            .values()
            .any(|session| !session.session.is_null())
            || !self.engine.is_null()
        {
            area.make_current();
        }
        let active_surface = self.active_surface_id;
        let active_pid = self
            .sessions
            .get(&active_surface)
            .filter(|session| !session.session.is_null())
            .map(|session| unsafe { ffi::kitty_session_child_pid(session.session) })
            .unwrap_or(0);
        let mut pids = Vec::new();
        for session in self.sessions.values_mut() {
            if session.pty_source != 0 {
                unsafe { g_source_remove(session.pty_source) };
                session.pty_source = 0;
            }
            if !session.session.is_null() {
                pids.push(unsafe { ffi::kitty_session_child_pid(session.session) });
                unsafe { ffi::kitty_session_close(session.session) };
                session.session = ptr::null_mut();
            }
            session.callback_ui = None;
        }
        if !self.engine.is_null() {
            unsafe { ffi::kitty_engine_shutdown(self.engine) };
            self.engine = ptr::null_mut();
        }
        let reaped = pids
            .iter()
            .all(|pid| *pid <= 0 || unsafe { libc::kill(*pid, 0) } != 0);
        diagnostic(
            "shutdown",
            &[
                ("pid", active_pid.to_string()),
                ("sessions", pids.len().to_string()),
                ("reaped", reaped.to_string()),
            ],
        );
    }
}

pub(crate) struct PumpContext {
    pub(crate) terminal: Weak<RefCell<Terminal>>,
    pub(crate) surface_id: SurfaceId,
}

unsafe extern "C" fn pump_pty(_fd: c_int, condition: u32, userdata: *mut c_void) -> c_int {
    let context = unsafe { &*(userdata.cast::<PumpContext>()) };
    let Some(terminal) = context.terminal.upgrade() else {
        return 0;
    };
    let Ok(mut terminal) = terminal.try_borrow_mut() else {
        return 1;
    };
    let surface_id = context.surface_id;
    let active = terminal.active_surface_id == surface_id;
    let (changed, bytes, area, child_alive, hidden_pump, resume_changed) = {
        let Some(session) = terminal.sessions.get_mut(&surface_id) else {
            // Unreachable in the registry design: close removes the source before the session.
            return 0;
        };
        if session.session.is_null() {
            return 0;
        }
        let changed = unsafe { ffi::kitty_session_pump(session.session) };
        let bytes = unsafe { ffi::kitty_session_last_pump_bytes(session.session) };
        let resume_changed = session
            .callback_ui
            .as_ref()
            .and_then(|ui| ui.pending_resume_command.borrow_mut().take())
            .map(|command| {
                session.resume_command = command;
                true
            })
            .unwrap_or(false);
        let hidden_pump = !active && bytes > 0 && !session.hidden_pump_reported;
        session.hidden_pump_reported |= hidden_pump;
        (
            changed,
            bytes,
            session
                .callback_ui
                .as_ref()
                .and_then(|ui| ui.visible.get().then(|| ui.area.upgrade()).flatten()),
            unsafe { ffi::kitty_session_child_alive(session.session) },
            hidden_pump,
            resume_changed,
        )
    };
    if resume_changed {
        diagnostic(
            "resume_metadata_captured",
            &[("surface", surface_id.to_string())],
        );
        if terminal.persist_state_now().is_err() {
            diagnostic(
                "state_save_failed",
                &[("reason", "resume-metadata".to_owned())],
            );
        }
    }
    if hidden_pump {
        diagnostic(
            "hidden_session_pumped",
            &[
                ("surface", surface_id.to_string()),
                ("bytes", bytes.to_string()),
            ],
        );
    }
    if changed || bytes > 0 {
        if active {
            terminal.refresh_cwd();
        }
        if let Some(area) = area {
            area.queue_render();
        }
    }
    if condition & (G_IO_ERR | G_IO_HUP | G_IO_NVAL) != 0 && !child_alive {
        if let Some(session) = terminal.sessions.get_mut(&surface_id) {
            session.pty_source = 0;
        }
        return 0;
    }
    1
}

unsafe extern "C" fn destroy_pump(userdata: *mut c_void) {
    drop(unsafe { Box::from_raw(userdata.cast::<PumpContext>()) });
}

pub(crate) fn attach_pty_source(
    terminal: &Rc<RefCell<Terminal>>,
    surface_id: SurfaceId,
    fd: c_int,
) -> Result<(), &'static str> {
    let context = Box::new(PumpContext {
        terminal: Rc::downgrade(terminal),
        surface_id,
    });
    let source = unsafe {
        g_unix_fd_add_full(
            PTY_SOURCE_PRIORITY,
            fd,
            G_IO_IN | G_IO_ERR | G_IO_HUP | G_IO_NVAL,
            Some(pump_pty),
            Box::into_raw(context).cast(),
            Some(destroy_pump),
        )
    };
    if source == 0 {
        return Err("pty-source");
    }
    terminal
        .borrow_mut()
        .sessions
        .get_mut(&surface_id)
        .unwrap()
        .pty_source = source;
    Ok(())
}

pub(crate) fn attach_missing_pty_sources(
    terminal: &Rc<RefCell<Terminal>>,
) -> Result<(), &'static str> {
    let pending = {
        let terminal = terminal.borrow();
        terminal
            .sessions
            .iter()
            .filter(|(_, session)| session.pty_source == 0 && !session.session.is_null())
            .map(|(surface, session)| (*surface, unsafe { ffi::kitty_session_fd(session.session) }))
            .collect::<Vec<_>>()
    };
    for (surface, fd) in pending {
        attach_pty_source(terminal, surface, fd)?;
    }
    Ok(())
}

pub(crate) fn attach_settings_source(terminal: &Rc<RefCell<Terminal>>) {
    if terminal.borrow().settings_source.is_some() {
        return;
    }
    let weak = Rc::downgrade(terminal);
    let source = glib::timeout_add_local(Duration::from_millis(250), move || {
        let Some(terminal) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if let Ok(mut terminal) = terminal.try_borrow_mut() {
            terminal.poll_settings();
        }
        glib::ControlFlow::Continue
    });
    terminal.borrow_mut().settings_source = Some(source);
}

pub(crate) fn attach_sigterm_source(
    terminal: &Rc<RefCell<Terminal>>,
    window: &ApplicationWindow,
    area: &GLArea,
) {
    let weak = Rc::downgrade(terminal);
    let window = window.downgrade();
    let area = area.downgrade();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        if !TERMINATION_REQUESTED.load(Ordering::Acquire) {
            return if weak.upgrade().is_some() {
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            };
        }
        let Some(terminal) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let Some(window) = window.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let Some(_area) = area.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let Ok(mut terminal_state) = terminal.try_borrow_mut() else {
            return glib::ControlFlow::Continue;
        };
        terminal_state.close_confirmed = true;
        diagnostic("sigterm_shutdown", &[]);
        drop(terminal_state);
        window.close();
        glib::ControlFlow::Break
    });
}
