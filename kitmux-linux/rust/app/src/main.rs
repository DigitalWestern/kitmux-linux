mod ffi;
mod window;

use ffi::{KitmuxGdkKeyInput, KitmuxKeyTracker, KitmuxKeyTranslation};
use gtk::gdk;
use gtk::gio;
use gtk::glib::{self, Propagation};
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Button, Entry, GLArea, Label, SearchBar};
use kitmux_model::{
    AppModel, AppSnapshot, CONTROL_DISPATCH_TIMEOUT, CloseOutcome, CommandId, ControlEventHistory,
    ControlMethod, ControlRequest, ControlResponse, ControlServer, ControlSocketError,
    DEFAULT_WHEEL_SCROLL_LINES, Direction, GroupId, GroupModel, LoadDisposition, NavigationTarget,
    PaneContainer, PaneContentKind, PaneDetail, PaneId, PaneRuntime, PaneSurface,
    PaneSurfaceDetail, PasteConfirmationReason, PixelRect, PixelSize, PollingFileWatcher,
    RestoreLayoutPolicy, ResumeCommandCurrentState, ResumeCommandIdentity,
    ResumeCommandSelectionPolicy, SETTINGS_MAX_BYTES, SNAPSHOT_VERSION, SettingsDocument,
    ShortcutAction, ShortcutChord, ShortcutMap, SplitAxis, SplitId, SplitLayout, SshProfile,
    SshProfileStore, SshProfileStoreError, SshResolution, SurfaceId, TabGroupSnapshot, TabId,
    TabModel, TerminalRuntime, TerminalTabSnapshot, WorkspaceId, WorkspaceModel, WorkspaceSnapshot,
    XdgPaths, command_palette_matches, detected_url, encode_control_response,
    load_settings_at_launch, load_state_at_launch, namespaced_number_target,
    paste_confirmation_reason, reload_settings, resolve_control_socket, save_settings, save_state,
    terminal_cell_scaled, valid_resume_command,
};
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::env;
use std::ffi::{CStr, CString, OsString, c_char, c_int, c_void};
use std::io::Read;
use std::ops::{Deref, DerefMut};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::ptr;
use std::rc::{Rc, Weak};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

const PTY_SOURCE_PRIORITY: c_int = 200;
const G_IO_IN: u32 = 1;
const G_IO_ERR: u32 = 8;
const G_IO_HUP: u32 = 16;
const G_IO_NVAL: u32 = 32;
const SPLIT_GAP: i32 = 4;
const MINIMUM_PANE: PixelSize = PixelSize::new(80, 50);
const SSH_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(5);
const SSH_RESOLUTION_MAX_OUTPUT: usize = 512 * 1024;
static UNSAFE_PASTE_COUNT: AtomicUsize = AtomicUsize::new(0);
static FOREGROUND_CLOSE_COUNT: AtomicUsize = AtomicUsize::new(0);
static TERMINATION_REQUESTED: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_sigterm(_: c_int) {
    TERMINATION_REQUESTED.store(true, Ordering::Release);
}

fn install_sigterm_handler() -> bool {
    unsafe {
        libc::signal(
            libc::SIGTERM,
            handle_sigterm as *const () as libc::sighandler_t,
        ) != libc::SIG_ERR
    }
}

thread_local! {
    static CONTROL_WAKE: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
}
const IMPLEMENTED_CONTROL_METHODS: &[ControlMethod] = &[
    ControlMethod::Ping,
    ControlMethod::Tree,
    ControlMethod::Identify,
    ControlMethod::Capabilities,
    ControlMethod::EventList,
    ControlMethod::WorkspaceCreate,
    ControlMethod::WorkspaceSelect,
    ControlMethod::WorkspaceRename,
    ControlMethod::WorkspaceMove,
    ControlMethod::WorkspaceClose,
    ControlMethod::GroupCreate,
    ControlMethod::GroupSelect,
    ControlMethod::GroupRename,
    ControlMethod::GroupMove,
    ControlMethod::GroupClose,
    ControlMethod::TabCreate,
    ControlMethod::TabSelect,
    ControlMethod::TabRename,
    ControlMethod::TabMove,
    ControlMethod::TabClose,
    ControlMethod::PaneSplit,
    ControlMethod::PaneFocus,
    ControlMethod::PaneMove,
    ControlMethod::PaneClose,
    ControlMethod::PaneSend,
    ControlMethod::PaneSendKey,
    ControlMethod::PaneReadScreen,
    ControlMethod::PaneNotify,
    ControlMethod::SshProfileList,
    ControlMethod::SshConnect,
    ControlMethod::SshReconnect,
];

#[cfg(test)]
mod control_surface_tests {
    use super::*;

    #[test]
    fn implemented_control_methods_are_catalogued() {
        assert_eq!(IMPLEMENTED_CONTROL_METHODS.len(), 31);
        assert!(
            IMPLEMENTED_CONTROL_METHODS
                .iter()
                .all(|method| ControlMethod::ALL.contains(method))
        );
        assert!(!IMPLEMENTED_CONTROL_METHODS.contains(&ControlMethod::PaneRename));
    }
}

#[repr(C)]
struct TerminalRegion {
    session: *mut ffi::KittySession,
    x: c_int,
    y: c_int,
    width: c_int,
    height: c_int,
    previous_width: c_int,
    previous_height: c_int,
    viewport_changed: bool,
}

unsafe extern "C" {
    fn g_unix_fd_add_full(
        priority: c_int,
        fd: c_int,
        condition: u32,
        callback: Option<unsafe extern "C" fn(c_int, u32, *mut c_void) -> c_int>,
        userdata: *mut c_void,
        destroy: Option<unsafe extern "C" fn(*mut c_void)>,
    ) -> u32;
    fn g_source_remove(source: u32) -> c_int;
    fn kitmux_terminal_render_regions(
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

fn diagnostic(event: &str, fields: &[(&str, String)]) {
    eprint!("kitmux event={event}");
    for (key, value) in fields {
        eprint!(" {key}={value}");
    }
    eprintln!();
}

struct RuntimeBundle {
    kitty_src: CString,
    libkitty_py: CString,
    python_home: CString,
    config: Option<CString>,
}

impl RuntimeBundle {
    fn discover() -> Result<Self, &'static str> {
        let environment = (
            env::var_os("KITTY_SRC"),
            env::var_os("LIBKITTY_PY"),
            env::var_os("PYTHONHOME"),
        );
        let (kitty_src, libkitty_py, python_home, config) = match environment {
            (Some(kitty), Some(glue), Some(python)) => (
                PathBuf::from(kitty),
                PathBuf::from(glue),
                PathBuf::from(python),
                env::var_os("LIBKITTY_TEST_CONFIG").map(PathBuf::from),
            ),
            (None, None, None) => {
                let executable = env::current_exe().map_err(|_| "executable")?;
                let root = executable
                    .parent()
                    .and_then(Path::parent)
                    .ok_or("runtime-root")?
                    .to_owned();
                (
                    root.clone(),
                    root.join("libkitty_py"),
                    root.clone(),
                    Some(root.join("etc/kitty.conf")),
                )
            }
            _ => return Err("partial-runtime-environment"),
        };
        if !kitty_src.join("kitty/fast_data_types.so").is_file() {
            return Err("kitty-runtime");
        }
        if !libkitty_py.join("glue.py").is_file() {
            return Err("libkitty-glue");
        }
        if !python_home.is_dir() {
            return Err("python-runtime");
        }
        Ok(Self {
            kitty_src: path_cstring(&kitty_src)?,
            libkitty_py: path_cstring(&libkitty_py)?,
            python_home: path_cstring(&python_home)?,
            config: config
                .filter(|path| path.is_file())
                .map(|path| path_cstring(&path))
                .transpose()?,
        })
    }
}

fn path_cstring(path: &Path) -> Result<CString, &'static str> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| "path-nul")
}

struct Account {
    home: PathBuf,
    shell: CString,
}

fn account() -> Account {
    unsafe {
        let uid = libc::geteuid();
        let mut entry: libc::passwd = std::mem::zeroed();
        let mut result = ptr::null_mut();
        let mut buffer = vec![0_u8; 16 * 1024];
        if libc::getpwuid_r(
            uid,
            &mut entry,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        ) == 0
            && !result.is_null()
        {
            let home = c_path(entry.pw_dir).filter(|path| path.is_absolute());
            let shell =
                c_path(entry.pw_shell).filter(|path| path.is_absolute() && is_executable(path));
            if let (Some(home), Some(shell)) = (home, shell)
                && let Ok(shell) = path_cstring(&shell)
            {
                return Account { home, shell };
            }
        }
    }
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    Account {
        home,
        shell: CString::new("/bin/sh").unwrap(),
    }
}

fn load_disposition_name(disposition: &LoadDisposition) -> &'static str {
    match disposition {
        LoadDisposition::Missing => "missing",
        LoadDisposition::Loaded => "loaded",
        LoadDisposition::SetAside(_) => "set-aside",
        LoadDisposition::RecoveredFromLastGood => "last-good",
        LoadDisposition::Unreadable => "unreadable",
    }
}

struct RestoredProduct {
    navigation: AppModel,
    active_surface: SurfaceId,
    surface_cwds: HashMap<SurfaceId, PathBuf>,
    surface_resume_commands: HashMap<SurfaceId, String>,
    surface_ssh_profiles: HashMap<SurfaceId, Uuid>,
    resume_offers: Vec<ResumeOffer>,
    created_workspaces: usize,
    created_groups: usize,
}

struct ResumeOffer {
    identity: ResumeCommandIdentity,
    location: String,
}

fn restored_product(snapshot: &AppSnapshot, home: &Path) -> Option<RestoredProduct> {
    let mut surface_ids = HashSet::new();
    let mut surface_cwds = HashMap::new();
    let mut surface_resume_commands = HashMap::new();
    let mut surface_ssh_profiles = HashMap::new();
    let mut resume_offers = Vec::new();
    let mut workspaces = Vec::with_capacity(snapshot.workspaces.len());
    for workspace in &snapshot.workspaces {
        let mut groups = Vec::with_capacity(workspace.tab_groups.len());
        for group in &workspace.tab_groups {
            let mut tabs = Vec::with_capacity(group.terminal_tabs.len());
            for (tab_index, tab) in group.terminal_tabs.iter().enumerate() {
                let mut panes = Vec::new();
                for (pane_index, pane_id) in tab.root.pane_ids().into_iter().enumerate() {
                    let detail = tab
                        .pane_details
                        .as_ref()
                        .and_then(|details| details.get(&pane_id.to_string()));
                    let mut surfaces = Vec::new();
                    let mut active_surface_index = 0;
                    if let Some(stack) = detail.and_then(|detail| detail.surfaces.as_ref()) {
                        let saved_active = detail
                            .and_then(|detail| detail.active_surface_index)
                            .unwrap_or(0) as usize;
                        for (index, saved) in stack.iter().enumerate() {
                            if saved.kind != PaneContentKind::Terminal {
                                continue;
                            }
                            let mut id = SurfaceId::from_uuid(saved.id);
                            while !surface_ids.insert(id) {
                                id = SurfaceId::new();
                            }
                            if index == saved_active {
                                active_surface_index = surfaces.len();
                            }
                            let cwd = saved
                                .cwd
                                .as_deref()
                                .map(PathBuf::from)
                                .filter(|path| valid_restored_cwd(path))
                                .unwrap_or_else(|| home.to_owned());
                            let cwd_label = cwd.to_string_lossy().into_owned();
                            surface_cwds.insert(id, cwd);
                            if let Some(command) = saved.resume_command.clone() {
                                surface_resume_commands.insert(id, command.clone());
                                resume_offers.push(ResumeOffer {
                                    identity: ResumeCommandIdentity {
                                        pane_id,
                                        surface_id: id,
                                        command,
                                        cwd: Some(cwd_label),
                                    },
                                    location: format!(
                                        "{} ▸ {} ▸ tab {} ▸ pane {} ▸ surface {}",
                                        workspace.name,
                                        group.name,
                                        tab_index + 1,
                                        pane_index + 1,
                                        surfaces.len() + 1
                                    ),
                                });
                            }
                            surfaces.push(PaneSurface::new(
                                id,
                                PaneRuntime::Terminal(Box::new(PendingTerminalRuntime {
                                    closed: false,
                                })),
                            ));
                        }
                    }
                    if surfaces.is_empty() {
                        let id = SurfaceId::new();
                        surface_ids.insert(id);
                        let cwd = detail
                            .and_then(|detail| detail.cwd.as_deref())
                            .map(PathBuf::from)
                            .filter(|path| valid_restored_cwd(path))
                            .unwrap_or_else(|| home.to_owned());
                        let cwd_label = cwd.to_string_lossy().into_owned();
                        surface_cwds.insert(id, cwd);
                        if let Some(profile_id) = detail.and_then(|detail| detail.ssh_profile_id) {
                            surface_ssh_profiles.insert(id, profile_id);
                        } else if let Some(command) =
                            detail.and_then(|detail| detail.resume_command.clone())
                        {
                            surface_resume_commands.insert(id, command.clone());
                            resume_offers.push(ResumeOffer {
                                identity: ResumeCommandIdentity {
                                    pane_id,
                                    surface_id: id,
                                    command,
                                    cwd: Some(cwd_label),
                                },
                                location: format!(
                                    "{} ▸ {} ▸ tab {} ▸ pane {}",
                                    workspace.name,
                                    group.name,
                                    tab_index + 1,
                                    pane_index + 1
                                ),
                            });
                        }
                        surfaces.push(PaneSurface::new(
                            id,
                            PaneRuntime::Terminal(Box::new(PendingTerminalRuntime {
                                closed: false,
                            })),
                        ));
                        active_surface_index = 0;
                    }
                    panes.push(PaneContainer::new(pane_id, surfaces, active_surface_index).ok()?);
                }
                let mut model =
                    TabModel::new(TabId::new(), tab.root.clone(), tab.focused_pane_id, panes)
                        .ok()?;
                if let Some(title) = tab.custom_title.as_deref() {
                    model.rename(Some(title));
                }
                tabs.push(model);
            }
            let mut model = GroupModel::new(
                GroupId::new(),
                tabs,
                group.active_terminal_tab_index as usize,
            )
            .ok()?;
            model.rename(&group.name);
            groups.push(model);
        }
        let mut model = WorkspaceModel::new(
            workspace.id.unwrap_or_default(),
            groups,
            workspace.active_tab_group_index as usize,
        )
        .ok()?;
        model.rename(&workspace.name);
        workspaces.push(model);
    }
    let navigation = AppModel::new(workspaces, snapshot.active_workspace_index as usize).ok()?;
    let active_surface = navigation
        .active_tab()
        .pane(navigation.active_tab().focused_pane_id())?
        .active_surface()
        .id();
    Some(RestoredProduct {
        navigation,
        active_surface,
        surface_cwds,
        surface_resume_commands,
        surface_ssh_profiles,
        resume_offers,
        created_workspaces: snapshot.created_workspace_count.max(1) as usize,
        created_groups: snapshot
            .workspaces
            .iter()
            .map(|workspace| workspace.created_group_count)
            .max()
            .unwrap_or(1)
            .max(1) as usize,
    })
}

fn valid_restored_cwd(path: &Path) -> bool {
    path.is_absolute()
        && path.is_dir()
        && path_cstring(path)
            .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::R_OK | libc::X_OK) == 0 })
}

unsafe fn c_path(value: *const c_char) -> Option<PathBuf> {
    if value.is_null() {
        return None;
    }
    let bytes = unsafe { CStr::from_ptr(value) }.to_bytes();
    (!bytes.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

fn is_executable(path: &Path) -> bool {
    let Ok(path) = path_cstring(path) else {
        return false;
    };
    unsafe { libc::access(path.as_ptr(), libc::X_OK) == 0 }
}

#[derive(Debug)]
enum SshRuntimeError {
    ExecutableNotFound,
    Launch,
    TimedOut,
    OutputTooLarge,
    CommandFailed,
    InvalidOutput,
}

impl std::fmt::Display for SshRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ExecutableNotFound => "the user's ssh executable was not found on PATH",
            Self::Launch => "ssh -G could not be launched",
            Self::TimedOut => "ssh -G timed out",
            Self::OutputTooLarge => "ssh -G returned too much output",
            Self::CommandFailed => "ssh -G failed",
            Self::InvalidOutput => "ssh -G returned incomplete output",
        })
    }
}

fn find_ssh_executable() -> Result<PathBuf, SshRuntimeError> {
    let path = env::var_os("PATH").unwrap_or_else(|| OsString::from("/usr/bin:/bin"));
    env::split_paths(&path)
        .map(|directory| directory.join("ssh"))
        .find_map(|candidate| {
            (candidate.is_file() && is_executable(&candidate))
                .then(|| std::fs::canonicalize(candidate).ok())
                .flatten()
        })
        .ok_or(SshRuntimeError::ExecutableNotFound)
}

fn read_limited(mut reader: impl Read) -> (Vec<u8>, bool) {
    let mut output = Vec::new();
    let mut too_large = false;
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                if output.len() < SSH_RESOLUTION_MAX_OUTPUT {
                    let keep = count.min(SSH_RESOLUTION_MAX_OUTPUT - output.len());
                    output.extend_from_slice(&buffer[..keep]);
                    if keep < count {
                        too_large = true;
                    }
                } else {
                    too_large = true;
                }
            }
            Err(_) => break,
        }
    }
    (output, too_large)
}

fn run_ssh_resolution(executable: &Path, host_alias: &str) -> Result<Vec<u8>, SshRuntimeError> {
    let mut child = Command::new(executable)
        .args(["-G", "--", host_alias])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| SshRuntimeError::Launch)?;
    let stdout = child.stdout.take().ok_or(SshRuntimeError::Launch)?;
    let stderr = child.stderr.take().ok_or(SshRuntimeError::Launch)?;
    let stdout_reader = thread::spawn(move || read_limited(stdout));
    let stderr_reader = thread::spawn(move || read_limited(stderr));
    let deadline = Instant::now() + SSH_RESOLUTION_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(SshRuntimeError::TimedOut);
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(SshRuntimeError::Launch);
            }
        }
    };
    let (output, output_too_large) = stdout_reader.join().unwrap_or((Vec::new(), false));
    let (_, error_too_large) = stderr_reader.join().unwrap_or((Vec::new(), false));
    if output_too_large || error_too_large {
        return Err(SshRuntimeError::OutputTooLarge);
    }
    status
        .success()
        .then_some(output)
        .ok_or(SshRuntimeError::CommandFailed)
}

fn resolve_ssh_profile(profile: &SshProfile) -> Result<(PathBuf, SshResolution), SshRuntimeError> {
    let executable = find_ssh_executable()?;
    let output = run_ssh_resolution(&executable, &profile.host_alias)?;
    let text = String::from_utf8(output).map_err(|_| SshRuntimeError::InvalidOutput)?;
    let resolution =
        SshResolution::parse(&profile.host_alias, &text).ok_or(SshRuntimeError::InvalidOutput)?;
    Ok((executable, resolution))
}

fn ssh_review_json(review: &kitmux_model::SshConnectionReview) -> serde_json::Value {
    json!({
        "fingerprint": review.fingerprint,
        "destination": review.destination,
        "hostAlias": review.host_alias,
        "remoteCommand": review.remote_command,
        "strictHostKeyChecking": review.strict_host_key_checking,
        "proxyJump": review.proxy_jump,
        "proxyCommand": review.proxy_command,
        "forwards": review.forwards,
        "hasExternallyListeningForward": review.has_externally_listening_forward,
        "requiresApproval": review.requires_approval,
    })
}

fn session_environment(account: &Account, ssh: bool) -> Vec<OsString> {
    let mut values = vec![
        OsString::from(format!("SHELL={}", account.shell.to_string_lossy())),
        OsString::from(format!("HOME={}", account.home.display())),
        OsString::from(format!(
            "PATH={}",
            env::var_os("PATH")
                .unwrap_or_else(|| OsString::from("/usr/bin:/bin"))
                .to_string_lossy()
        )),
        OsString::from("COLORTERM=truecolor"),
        OsString::from("TERM=xterm-256color"),
    ];
    for (key, value) in env::vars_os() {
        let key = key.to_string_lossy();
        if key == "USER" || key == "LOGNAME" || key == "LANG" || key.starts_with("LC_") {
            values.push(OsString::from(format!("{key}={}", value.to_string_lossy())));
        }
    }
    if ssh {
        if let Some(agent) = env::var_os("SSH_AUTH_SOCK").filter(|value| !value.is_empty()) {
            values.push(OsString::from(format!(
                "SSH_AUTH_SOCK={}",
                agent.to_string_lossy()
            )));
            diagnostic("ssh_agent", &[("available", "true".to_owned())]);
        } else {
            diagnostic("ssh_agent", &[("available", "false".to_owned())]);
        }
    }
    values
}

struct CallbackUi {
    window: glib::WeakRef<ApplicationWindow>,
    area: glib::WeakRef<GLArea>,
    status: glib::WeakRef<Label>,
    visible: Cell<bool>,
    disconnected: Cell<bool>,
    close_window_on_exit: bool,
    pending_resume_command: RefCell<Option<Option<String>>>,
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

struct SessionState {
    session: *mut ffi::KittySession,
    callback_ui: Option<Box<CallbackUi>>,
    pty_source: u32,
    framebuffer_width: c_int,
    framebuffer_height: c_int,
    cell_width: c_int,
    cell_height: c_int,
    last_cwd: Option<PathBuf>,
    keys: KitmuxKeyTracker,
    im_consumed: KitmuxKeyTracker,
    preedit_active: bool,
    filtering: bool,
    filtering_had_preedit: bool,
    filtering_committed: bool,
    filtering_encoded: bool,
    filtering_input: KitmuxGdkKeyInput,
    scroll_residue: f64,
    selection_active: bool,
    mouse_reporting_button: Option<c_int>,
    hidden_pump_reported: bool,
    ssh_profile_id: Option<Uuid>,
    resume_command: Option<String>,
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

struct Terminal {
    engine: *mut ffi::KittyEngine,
    sessions: HashMap<SurfaceId, SessionState>,
    active_surface_id: SurfaceId,
    xdg: Option<XdgPaths>,
    shortcuts: ShortcutMap,
    default_font_size: f64,
    shortcut_consumed: KitmuxKeyTracker,
    close_confirmed: bool,
    modal_dialog_open: bool,
    paste_confirmation_threshold: usize,
    wheel_scroll_lines: u64,
    confirm_close_with_running_process: bool,
    persistence: Option<PersistenceState>,
    settings_source: Option<glib::SourceId>,
    account_home: PathBuf,
    workspace_id: WorkspaceId,
    pane_id: PaneId,
    navigation: Option<AppModel>,
    navigation_ui: Option<NavigationUi>,
    created_workspaces: usize,
    created_groups: usize,
    divider_drag: Option<(SplitId, f64, f64)>,
    control_server: Option<ControlServer>,
    control_notice: Option<String>,
    control_history: ControlEventHistory,
    ssh_profiles: Option<SshProfileStore>,
    pending_resume_offers: Vec<ResumeOffer>,
}

struct PendingControlCall {
    request: ControlRequest,
    peer_uid: u32,
    response: SyncSender<ControlResponse>,
}

struct PendingTerminalRuntime {
    closed: bool,
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

#[derive(Clone)]
struct NavigationUi {
    sidebar_shell: glib::WeakRef<gtk::Box>,
    sidebar: glib::WeakRef<gtk::Box>,
    tab_strip: glib::WeakRef<gtk::Box>,
    group_label: glib::WeakRef<Label>,
    status: glib::WeakRef<Label>,
    window: glib::WeakRef<ApplicationWindow>,
    area: glib::WeakRef<GLArea>,
    search_bar: glib::WeakRef<SearchBar>,
    search_entry: glib::WeakRef<Entry>,
    command_palette: glib::WeakRef<Button>,
    settings: glib::WeakRef<Button>,
}

#[derive(Clone, Copy)]
enum RenameTarget {
    Workspace(WorkspaceId),
    Group(GroupId),
    Tab(TabId),
}

enum NavigationEffect {
    Changed,
    Rejected,
    CloseWindow,
    Rename(RenameTarget),
}

#[derive(Clone, Copy)]
enum ForegroundScope {
    Pane,
    Tab,
    Group,
    Workspace,
}

struct PersistenceState {
    state_path: PathBuf,
    state_may_write: bool,
    settings_path: PathBuf,
    settings: SettingsDocument,
    settings_watcher: Option<PollingFileWatcher>,
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

fn changed(value: bool) -> NavigationEffect {
    if value {
        NavigationEffect::Changed
    } else {
        NavigationEffect::Rejected
    }
}

fn foreground_scope(command: CommandId) -> ForegroundScope {
    match command {
        CommandId::PaneClose => ForegroundScope::Pane,
        CommandId::GroupClose => ForegroundScope::Group,
        CommandId::WorkspaceClose => ForegroundScope::Workspace,
        _ => ForegroundScope::Pane,
    }
}

fn install_control_server(terminal: &Rc<RefCell<Terminal>>) -> Result<(), ControlSocketError> {
    let environment: HashMap<String, String> = env::vars().collect();
    let address = resolve_control_socket(&environment, unsafe { libc::geteuid() })
        .map_err(|error| ControlSocketError::Path(error.to_string()))?;
    let queue = Arc::new(Mutex::new(VecDeque::new()));
    let wake_context = glib::MainContext::default();
    let wake_pending = Arc::new(AtomicBool::new(false));
    let history = terminal.borrow().control_history.clone();
    let handler_queue = Arc::clone(&queue);
    let handler_history = history.clone();
    let handler_wake_context = wake_context.clone();
    let handler_wake_pending = Arc::clone(&wake_pending);
    let server = ControlServer::start(address.clone(), history.clone(), move |request, peer| {
        let (sender, receiver) = mpsc::sync_channel(1);
        let request_id = request.id.clone();
        let request_method = request.method.clone();
        let mut queue = handler_queue
            .lock()
            .expect("control dispatch queue lock poisoned");
        if queue.len() >= 128 {
            handler_history.record(&request_method, &request_id, false, peer.uid);
            return ControlResponse::failure(request_id, "busy", "control dispatch queue is full");
        }
        queue.push_back(PendingControlCall {
            request,
            peer_uid: peer.uid,
            response: sender,
        });
        drop(queue);
        if !handler_wake_pending.swap(true, Ordering::AcqRel) {
            handler_wake_context.invoke(|| {
                CONTROL_WAKE.with(|wake| {
                    if let Some(dispatch) = wake.borrow().as_ref() {
                        dispatch();
                    }
                });
            });
        }
        match receiver.recv_timeout(CONTROL_DISPATCH_TIMEOUT) {
            Ok(response) => response,
            Err(_) => {
                handler_history.record(&request_method, &request_id, false, peer.uid);
                ControlResponse::failure(request_id, "timeout", "control request timed out")
            }
        }
    })?;
    let weak = Rc::downgrade(terminal);
    let dispatch_queue = Arc::clone(&queue);
    let dispatch_history = history.clone();
    let dispatch_pending = Arc::clone(&wake_pending);
    CONTROL_WAKE.with(|wake| {
        *wake.borrow_mut() = Some(Box::new(move || {
            dispatch_pending.store(false, Ordering::Release);
            let Some(terminal) = weak.upgrade() else {
                return;
            };
            let calls = dispatch_queue
                .lock()
                .expect("control dispatch queue lock poisoned")
                .drain(..)
                .collect::<Vec<_>>();
            for call in calls {
                let method = call.request.method.clone();
                let response = dispatch_control_request(&terminal, call.request, &dispatch_history);
                dispatch_history.record(&method, &response.id, response.ok, call.peer_uid);
                let _ = call.response.send(response);
            }
        }));
    });
    let mut terminal = terminal.borrow_mut();
    terminal.control_server = Some(server);
    diagnostic(
        "control_server_ready",
        &[
            ("socket", address.path().display().to_string()),
            ("mode", "600".to_owned()),
        ],
    );
    Ok(())
}

fn control_success(request: &ControlRequest, result: serde_json::Value) -> ControlResponse {
    ControlResponse::success(request.id.clone(), result)
}

fn control_failure(
    request: &ControlRequest,
    code: &str,
    message: impl Into<String>,
) -> ControlResponse {
    ControlResponse::failure(request.id.clone(), code, message)
}

fn dispatch_control_request(
    terminal: &Rc<RefCell<Terminal>>,
    request: ControlRequest,
    history: &ControlEventHistory,
) -> ControlResponse {
    let Some(method) = request.method_id() else {
        return control_failure(
            &request,
            "unknown_method",
            "method is not in the control catalog",
        );
    };
    if terminal.borrow().modal_dialog_open
        && !matches!(
            method,
            ControlMethod::Ping
                | ControlMethod::Identify
                | ControlMethod::Capabilities
                | ControlMethod::Tree
                | ControlMethod::EventList
                | ControlMethod::PaneReadScreen
        )
    {
        return control_failure(
            &request,
            "busy",
            "control mutations are paused while a modal dialog is open",
        );
    }
    if !matches!(
        method,
        ControlMethod::Ping
            | ControlMethod::Identify
            | ControlMethod::Capabilities
            | ControlMethod::EventList
    ) && terminal.borrow().navigation.is_none()
    {
        return control_failure(&request, "not_ready", "Kitmux is still initializing");
    }
    match method {
        ControlMethod::Ping => control_success(&request, json!({"message": "pong"})),
        ControlMethod::Identify => control_success(
            &request,
            json!({
                "pid": std::process::id(),
                "uid": unsafe { libc::geteuid() },
                "version": env!("CARGO_PKG_VERSION")
            }),
        ),
        ControlMethod::Capabilities => control_success(
            &request,
            json!({
                "protocolVersion": 1,
                "methods": ControlMethod::ALL.iter().map(|method| method.as_str()).collect::<Vec<_>>(),
                "implemented": IMPLEMENTED_CONTROL_METHODS.iter().map(|method| method.as_str()).collect::<Vec<_>>()
            }),
        ),
        ControlMethod::Tree => {
            let snapshot = terminal.borrow().snapshot();
            match serde_json::to_value(snapshot) {
                Ok(value) => control_success(&request, value),
                Err(error) => control_failure(&request, "internal_error", error.to_string()),
            }
        }
        ControlMethod::EventList => {
            let after = parse_u64(&request, "after").unwrap_or(0);
            let limit = parse_usize(&request, "limit").unwrap_or(100);
            let category = request.params.get("category").map(String::as_str);
            let events = history.list(after, limit, category);
            let cursor = history.cursor();
            control_success(&request, json!({"events": events, "eventCursor": cursor}))
        }
        ControlMethod::WorkspaceCreate => {
            control_navigation(terminal, &request, CommandId::WorkspaceNew)
        }
        ControlMethod::GroupCreate => control_navigation(terminal, &request, CommandId::GroupNew),
        ControlMethod::TabCreate => {
            control_navigation(terminal, &request, CommandId::TerminalNewTab)
        }
        ControlMethod::WorkspaceSelect => control_select(terminal, &request, "workspace"),
        ControlMethod::GroupSelect => control_select(terminal, &request, "group"),
        ControlMethod::TabSelect => control_select(terminal, &request, "tab"),
        ControlMethod::WorkspaceRename => control_rename(terminal, &request, "workspace"),
        ControlMethod::GroupRename => control_rename(terminal, &request, "group"),
        ControlMethod::TabRename => control_rename(terminal, &request, "tab"),
        ControlMethod::WorkspaceMove => control_move(terminal, &request, "workspace"),
        ControlMethod::GroupMove => control_move(terminal, &request, "group"),
        ControlMethod::TabMove => control_move(terminal, &request, "tab"),
        ControlMethod::WorkspaceClose => {
            control_close(terminal, &request, "workspace", ForegroundScope::Workspace)
        }
        ControlMethod::GroupClose => {
            control_close(terminal, &request, "group", ForegroundScope::Group)
        }
        ControlMethod::TabClose => control_close(terminal, &request, "tab", ForegroundScope::Tab),
        ControlMethod::PaneSplit => {
            let axis = match request.params.get("axis").map(String::as_str) {
                Some("right") => CommandId::PaneSplitRight,
                Some("down") => CommandId::PaneSplitDown,
                _ => {
                    return control_failure(
                        &request,
                        "invalid_params",
                        "pane.split axis must be right or down",
                    );
                }
            };
            if let Some(id) = request.params.get("id")
                && !select_pane(terminal, id)
            {
                return control_failure(&request, "not_found", "pane was not found");
            }
            control_navigation(terminal, &request, axis)
        }
        ControlMethod::PaneFocus => {
            let Some(id) = request.params.get("id") else {
                return control_failure(&request, "invalid_params", "pane.focus requires id");
            };
            let changed = select_pane(terminal, id);
            if changed {
                apply_navigation_effect(terminal, NavigationEffect::Changed);
                control_success(&request, json!({"changed": true}))
            } else {
                control_failure(&request, "not_found", "pane was not found")
            }
        }
        ControlMethod::PaneRename => control_failure(
            &request,
            "unsupported_method",
            "pane names are not stored by the Linux model",
        ),
        ControlMethod::PaneMove => {
            let Some(target) = request.params.get("target") else {
                return control_failure(&request, "invalid_params", "pane.move requires target");
            };
            let current = request
                .params
                .get("id")
                .map(String::as_str)
                .unwrap_or("current");
            if !select_pane(terminal, current) {
                return control_failure(&request, "not_found", "pane was not found");
            }
            let Ok(target) = PaneId::from_str(target) else {
                return control_failure(&request, "invalid_params", "target must be a pane ID");
            };
            let changed = {
                let mut terminal = terminal.borrow_mut();
                let Some(navigation) = terminal.navigation.as_mut() else {
                    return control_failure(&request, "not_ready", "navigation is not ready");
                };
                let current = navigation.active_tab().focused_pane_id();
                navigation.active_tab_mut().swap_panes(current, target)
            };
            if changed {
                apply_navigation_effect(terminal, NavigationEffect::Changed);
                control_success(&request, json!({"changed": true}))
            } else {
                control_failure(&request, "not_found", "target pane was not found")
            }
        }
        ControlMethod::PaneClose => {
            control_close(terminal, &request, "pane", ForegroundScope::Pane)
        }
        ControlMethod::PaneSend => control_send(terminal, &request),
        ControlMethod::PaneSendKey => control_send_key(terminal, &request),
        ControlMethod::PaneReadScreen => control_read_screen(terminal, &request),
        ControlMethod::PaneNotify => {
            let message = request.params.get("message").cloned().unwrap_or_default();
            if message.is_empty() {
                control_failure(&request, "invalid_params", "pane.notify requires message")
            } else {
                diagnostic("control_notify", &[("bytes", message.len().to_string())]);
                control_success(&request, json!({"message": "notification accepted"}))
            }
        }
        ControlMethod::SshProfileList => control_ssh_profile_list(terminal, &request),
        ControlMethod::SshConnect => control_ssh_connect(terminal, &request),
        ControlMethod::SshReconnect => control_ssh_reconnect(terminal, &request),
        _ => control_failure(
            &request,
            "unsupported_method",
            "method is reserved for a later Phase 6 slice",
        ),
    }
}

fn ssh_uuid_param(request: &ControlRequest, key: &str) -> Option<Uuid> {
    Uuid::parse_str(request.params.get(key)?).ok()
}

fn valid_ssh_fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn control_ssh_profile_list(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
) -> ControlResponse {
    if !request.params.is_empty() {
        return control_failure(
            request,
            "invalid_params",
            "ssh.profile.list takes no parameters",
        );
    }
    let profiles = terminal
        .borrow()
        .ssh_profiles
        .as_ref()
        .map(|store| {
            store
                .list()
                .into_iter()
                .map(|profile| {
                    json!({
                        "id": profile.id.to_string(),
                        "name": profile.name,
                        "hostAlias": profile.host_alias,
                        "hasRemoteCommand": profile.remote_command.is_some(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    control_success(request, json!({"sshProfiles": profiles}))
}

fn control_ssh_connect(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
) -> ControlResponse {
    control_ssh_action(terminal, request, false)
}

fn control_ssh_reconnect(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
) -> ControlResponse {
    control_ssh_action(terminal, request, true)
}

fn control_ssh_action(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
    reconnect: bool,
) -> ControlResponse {
    let required = if reconnect { "pane" } else { "profile" };
    if request
        .params
        .keys()
        .any(|key| key != required && key != "fingerprint")
    {
        return control_failure(request, "invalid_params", "unknown SSH parameter");
    }
    let target = match ssh_uuid_param(request, required) {
        Some(value) => value,
        None => {
            return control_failure(
                request,
                "invalid_params",
                "SSH actions require an exact UUID",
            );
        }
    };
    let surface = if reconnect {
        let Some(surface) = resolve_pane_surface(terminal, &target.to_string()) else {
            return control_failure(request, "not_found", "SSH pane was not found");
        };
        let terminal_state = terminal.borrow();
        let Some(session) = terminal_state.sessions.get(&surface) else {
            return control_failure(request, "not_found", "SSH pane was not found");
        };
        if session.ssh_profile_id.is_none() {
            return control_failure(request, "invalid_params", "pane is not an SSH surface");
        }
        let disconnected = session
            .callback_ui
            .as_ref()
            .is_some_and(|callback| callback.disconnected.get());
        if !disconnected
            && !session.session.is_null()
            && unsafe { ffi::kitty_session_child_alive(session.session) }
        {
            return control_failure(request, "busy", "SSH pane is still connected");
        }
        Some(surface)
    } else {
        None
    };
    let profile_id = if let Some(surface) = surface {
        terminal
            .borrow()
            .sessions
            .get(&surface)
            .and_then(|session| session.ssh_profile_id)
            .expect("SSH surface has a profile ID")
    } else {
        target
    };
    let profile = terminal
        .borrow()
        .ssh_profiles
        .as_ref()
        .and_then(|store| store.profile(profile_id));
    let Some(profile) = profile else {
        return control_failure(request, "not_found", "SSH profile was not found");
    };
    let (executable, resolution) = match resolve_ssh_profile(&profile) {
        Ok(value) => value,
        Err(error) => return control_failure(request, "resolution_failed", error.to_string()),
    };
    let review = resolution.review(&profile);
    let requested_fingerprint = request.params.get("fingerprint");
    if let Some(fingerprint) = requested_fingerprint {
        if !valid_ssh_fingerprint(fingerprint) {
            return control_failure(
                request,
                "invalid_params",
                "fingerprint must be lowercase SHA-256",
            );
        }
        if fingerprint != &review.fingerprint {
            return control_failure(
                request,
                "stale_review",
                "SSH resolution changed; review the new fingerprint",
            );
        }
    }
    if review.requires_approval && requested_fingerprint != Some(&review.fingerprint) {
        return control_success(
            request,
            json!({
                "connected": false,
                "approvalRequired": true,
                "review": ssh_review_json(&review),
            }),
        );
    }
    if review.requires_approval {
        let approval = terminal
            .borrow()
            .ssh_profiles
            .as_ref()
            .ok_or(SshProfileStoreError::InvalidDocument)
            .and_then(|store| store.approve(profile.id, &review.fingerprint));
        if let Err(error) = approval {
            return control_failure(request, "store_failed", error.to_string());
        }
    }
    if let Some(surface) = surface {
        let replaced = terminal
            .borrow_mut()
            .replace_ssh_surface(surface, &profile, &executable);
        if !replaced {
            return control_failure(request, "launch_failed", "SSH reconnect could not start");
        }
        let _ = attach_missing_pty_sources(terminal);
        control_success(
            request,
            json!({
                "connected": true,
                "reconnected": true,
                "destination": review.destination,
                "pane": target.to_string(),
            }),
        )
    } else {
        let created = terminal.borrow_mut().create_ssh_tab(&profile, &executable);
        if !created {
            return control_failure(request, "launch_failed", "SSH tab could not start");
        }
        apply_navigation_effect(terminal, NavigationEffect::Changed);
        let pane = terminal
            .borrow()
            .navigation
            .as_ref()
            .map(|navigation| navigation.active_tab().focused_pane_id().to_string());
        control_success(
            request,
            json!({
                "connected": true,
                "destination": review.destination,
                "pane": pane,
            }),
        )
    }
}

fn control_navigation(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
    command: CommandId,
) -> ControlResponse {
    let effect = terminal.borrow_mut().navigation_action(command);
    let accepted = matches!(
        effect,
        NavigationEffect::Changed | NavigationEffect::CloseWindow
    );
    if accepted {
        apply_navigation_effect(terminal, effect);
        control_success(request, json!({"changed": true}))
    } else {
        control_failure(request, "rejected", "navigation command was rejected")
    }
}

fn control_select(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
    noun: &str,
) -> ControlResponse {
    let Some(id) = request.params.get("id") else {
        return control_failure(request, "invalid_params", "selection requires id");
    };
    let changed = match noun {
        "workspace" => select_workspace(terminal, id),
        "group" => select_group(terminal, id),
        "tab" => select_tab(terminal, id),
        _ => false,
    };
    if changed {
        apply_navigation_effect(terminal, NavigationEffect::Changed);
        control_success(request, json!({"changed": true}))
    } else {
        control_failure(request, "not_found", format!("{noun} was not found"))
    }
}

fn control_rename(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
    noun: &str,
) -> ControlResponse {
    let Some(name) = request.params.get("name") else {
        return control_failure(request, "invalid_params", "rename requires name");
    };
    if name.trim().is_empty() || name.chars().any(char::is_control) {
        return control_failure(
            request,
            "invalid_params",
            "name is empty or contains controls",
        );
    }
    let id = request
        .params
        .get("id")
        .map(String::as_str)
        .unwrap_or("current");
    let previous_selection = navigation_selection(terminal);
    if !control_target_exists(terminal, noun, id) {
        return control_failure(request, "not_found", format!("{noun} was not found"));
    }
    let selected = match noun {
        "workspace" => select_workspace(terminal, id),
        "group" => select_group(terminal, id),
        "tab" => select_tab(terminal, id),
        _ => false,
    };
    if !selected {
        return control_failure(request, "not_ready", "navigation is not ready");
    }
    let changed = {
        let mut terminal = terminal.borrow_mut();
        let Some(navigation) = terminal.navigation.as_mut() else {
            return control_failure(request, "not_ready", "navigation is not ready");
        };
        match noun {
            "workspace" => navigation.active_workspace_mut().rename(name),
            "group" => navigation
                .active_workspace_mut()
                .active_group_mut()
                .rename(name),
            "tab" => navigation.active_tab_mut().rename(Some(name)),
            _ => false,
        }
    };
    if changed {
        apply_navigation_effect(terminal, NavigationEffect::Changed);
        control_success(request, json!({"changed": true}))
    } else {
        restore_navigation(terminal, previous_selection);
        control_failure(request, "invalid_params", "name is unchanged")
    }
}

fn control_move(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
    noun: &str,
) -> ControlResponse {
    let Some(id) = request.params.get("id") else {
        return control_failure(request, "invalid_params", "move requires id");
    };
    let Some(index) = request
        .params
        .get("index")
        .and_then(|value| value.parse::<usize>().ok())
    else {
        return control_failure(request, "invalid_params", "move requires a numeric index");
    };
    let changed = {
        let mut terminal = terminal.borrow_mut();
        let Some(navigation) = terminal.navigation.as_mut() else {
            return control_failure(request, "not_ready", "navigation is not ready");
        };
        match noun {
            "workspace" => navigation
                .workspaces()
                .iter()
                .find(|item| item.id().to_string() == *id)
                .map(|item| item.id())
                .is_some_and(|id| navigation.move_workspace(id, index)),
            "group" => {
                let group = navigation
                    .active_workspace()
                    .groups()
                    .iter()
                    .find(|item| item.id().to_string() == *id)
                    .map(|item| item.id());
                group.is_some_and(|id| navigation.active_workspace_mut().move_group(id, index))
            }
            "tab" => {
                let tab = navigation
                    .active_workspace()
                    .active_group()
                    .tabs()
                    .iter()
                    .find(|item| item.id().to_string() == *id)
                    .map(|item| item.id());
                tab.is_some_and(|id| {
                    navigation
                        .active_workspace_mut()
                        .active_group_mut()
                        .move_tab(id, index)
                })
            }
            _ => false,
        }
    };
    if changed {
        apply_navigation_effect(terminal, NavigationEffect::Changed);
        control_success(request, json!({"changed": true}))
    } else {
        control_failure(request, "rejected", format!("{noun} move was rejected"))
    }
}

fn control_close(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
    noun: &str,
    scope: ForegroundScope,
) -> ControlResponse {
    let id = request
        .params
        .get("id")
        .map(String::as_str)
        .unwrap_or("current");
    let previous_selection = navigation_selection(terminal);
    if !control_target_exists(terminal, noun, id) {
        return control_failure(request, "not_found", format!("{noun} was not found"));
    }
    let selected = match noun {
        "workspace" => select_workspace(terminal, id),
        "group" => select_group(terminal, id),
        "tab" => select_tab(terminal, id),
        "pane" => select_pane(terminal, id),
        _ => false,
    };
    if !selected {
        return control_failure(request, "not_ready", "navigation is not ready");
    }
    let force = request
        .params
        .get("force")
        .is_some_and(|value| value == "true");
    let previous_close_confirmed = terminal.borrow().close_confirmed;
    let foreground = terminal.borrow().foreground_surfaces(Some(scope));
    if !foreground.is_empty() && !force {
        restore_navigation(terminal, previous_selection);
        return control_failure(
            request,
            "confirmation_required",
            "a foreground process is running; retry with force=true",
        );
    }
    if force {
        terminal.borrow_mut().close_confirmed = true;
    }
    let mut applied_effect = false;
    let changed = match noun {
        "pane" => {
            let effect = terminal
                .borrow_mut()
                .navigation_action(CommandId::PaneClose);
            let changed = matches!(
                effect,
                NavigationEffect::Changed | NavigationEffect::CloseWindow
            );
            if changed {
                apply_navigation_effect(terminal, effect);
                applied_effect = true;
            }
            changed
        }
        "tab" => {
            let mut terminal = terminal.borrow_mut();
            let Some(navigation) = terminal.navigation.as_mut() else {
                terminal.close_confirmed = previous_close_confirmed;
                return control_failure(request, "not_ready", "navigation is not ready");
            };
            let index = navigation
                .active_workspace()
                .active_group()
                .active_tab_index();
            navigation
                .active_workspace_mut()
                .active_group_mut()
                .close_tab(index)
                .is_some()
        }
        "group" => {
            let mut terminal = terminal.borrow_mut();
            let Some(navigation) = terminal.navigation.as_mut() else {
                terminal.close_confirmed = previous_close_confirmed;
                return control_failure(request, "not_ready", "navigation is not ready");
            };
            let index = navigation.active_workspace().active_group_index();
            navigation
                .active_workspace_mut()
                .close_group(index)
                .is_some()
        }
        "workspace" => {
            let mut terminal = terminal.borrow_mut();
            let Some(navigation) = terminal.navigation.as_mut() else {
                terminal.close_confirmed = previous_close_confirmed;
                return control_failure(request, "not_ready", "navigation is not ready");
            };
            navigation
                .close_workspace(navigation.active_workspace_index())
                .is_some()
        }
        _ => false,
    };
    terminal.borrow_mut().close_confirmed = previous_close_confirmed;
    if changed && !applied_effect {
        apply_navigation_effect(terminal, NavigationEffect::Changed);
        control_success(request, json!({"changed": true}))
    } else {
        restore_navigation(terminal, previous_selection);
        control_failure(request, "rejected", format!("{noun} close was rejected"))
    }
}

fn control_send(terminal: &Rc<RefCell<Terminal>>, request: &ControlRequest) -> ControlResponse {
    let Some(text) = request.params.get("text") else {
        return control_failure(request, "invalid_params", "pane.send requires text");
    };
    let id = request
        .params
        .get("id")
        .map(String::as_str)
        .unwrap_or("current");
    if !select_pane(terminal, id) {
        return control_failure(request, "not_found", "pane was not found");
    }
    apply_navigation_effect(terminal, NavigationEffect::Changed);
    if !active_surface_matches_pane(terminal, id) {
        return control_failure(
            request,
            "not_ready",
            "target pane is not the reconciled active surface",
        );
    }
    let force = request
        .params
        .get("force")
        .is_some_and(|value| value == "true");
    if !force {
        let threshold = terminal.borrow().paste_confirmation_threshold;
        if let Some(reason) = paste_confirmation_reason(text, threshold) {
            return control_failure(
                request,
                "confirmation_required",
                format!("pane.send requires confirmation ({})", paste_reason(reason)),
            );
        }
    }
    terminal.borrow_mut().paste(text);
    control_success(request, json!({"byteCount": text.len()}))
}

fn control_send_key(terminal: &Rc<RefCell<Terminal>>, request: &ControlRequest) -> ControlResponse {
    let Some(key) = request.params.get("key") else {
        return control_failure(request, "invalid_params", "pane.send_key requires key");
    };
    let id = request
        .params
        .get("id")
        .map(String::as_str)
        .unwrap_or("current");
    if !select_pane(terminal, id) {
        return control_failure(request, "not_found", "pane was not found");
    }
    apply_navigation_effect(terminal, NavigationEffect::Changed);
    if !active_surface_matches_pane(terminal, id) {
        return control_failure(
            request,
            "not_ready",
            "target pane is not the reconciled active surface",
        );
    }
    let bytes = match key.as_str() {
        "Enter" => vec![b'\r'],
        "Tab" => vec![b'\t'],
        "Escape" => vec![0x1b],
        "Backspace" => vec![0x7f],
        value
            if value.chars().count() == 1
                && !value.chars().next().expect("one character").is_control() =>
        {
            value.as_bytes().to_vec()
        }
        _ => {
            return control_failure(
                request,
                "invalid_params",
                "key is not a supported safe keystroke",
            );
        }
    };
    let terminal = terminal.borrow();
    if terminal.session.is_null() {
        return control_failure(request, "not_ready", "terminal session is not ready");
    }
    unsafe { ffi::kitty_session_write(terminal.session, bytes.as_ptr().cast(), bytes.len()) };
    control_success(request, json!({"byteCount": bytes.len()}))
}

fn control_read_screen(
    terminal: &Rc<RefCell<Terminal>>,
    request: &ControlRequest,
) -> ControlResponse {
    let id = request
        .params
        .get("id")
        .map(String::as_str)
        .unwrap_or("current");
    let Some(surface) = resolve_pane_surface(terminal, id) else {
        return control_failure(request, "not_found", "pane was not found");
    };
    let lines = match request.params.get("lines") {
        Some(value) => match value.parse::<usize>() {
            Ok(lines) => Some(lines),
            Err(_) => {
                return control_failure(request, "invalid_params", "lines must be a number");
            }
        },
        None => None,
    };
    let terminal = terminal.borrow();
    let Some(session) = terminal.sessions.get(&surface) else {
        return control_failure(request, "not_ready", "terminal session is not ready");
    };
    if session.session.is_null() {
        return control_failure(request, "not_ready", "terminal session is not ready");
    }
    let Some(text) = owned_c_string(unsafe { ffi::kitty_session_text(session.session) }) else {
        return control_failure(
            request,
            "internal_error",
            "terminal screen text was unavailable",
        );
    };
    let total = text.len();
    let selected = lines.map_or_else(|| text.clone(), |count| tail_screen_lines(&text, count));
    let line_truncated = selected.len() != text.len();
    let mut low = 0;
    let mut high = selected.len();
    let mut bounded = String::new();
    while low <= high {
        let midpoint = low + (high - low) / 2;
        let candidate = utf8_prefix(&selected, midpoint);
        let response = screen_response(
            request,
            &candidate,
            total,
            line_truncated || candidate.len() != selected.len(),
        );
        if encode_control_response(&response).is_ok() {
            bounded = candidate;
            low = midpoint + 1;
        } else if midpoint == 0 {
            break;
        } else {
            high = midpoint - 1;
        }
    }
    let truncated = line_truncated || bounded.len() != selected.len();
    control_success(
        request,
        json!({
            "text": bounded,
            "byteCount": bounded.len(),
            "totalByteCount": total,
            "truncated": truncated
        }),
    )
}

fn screen_response(
    request: &ControlRequest,
    text: &str,
    total_byte_count: usize,
    truncated: bool,
) -> ControlResponse {
    control_success(
        request,
        json!({
            "text": text,
            "byteCount": text.len(),
            "totalByteCount": total_byte_count,
            "truncated": truncated
        }),
    )
}

fn tail_screen_lines(text: &str, count: usize) -> String {
    let lines = text.split_inclusive('\n').collect::<Vec<_>>();
    let start = lines.len().saturating_sub(count);
    lines[start..].concat()
}

fn utf8_prefix(text: &str, maximum_bytes: usize) -> String {
    if maximum_bytes >= text.len() {
        return text.to_owned();
    }
    let end = text
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= maximum_bytes)
        .last()
        .unwrap_or(0);
    text[..end].to_owned()
}

fn parse_u64(request: &ControlRequest, key: &str) -> Option<u64> {
    request.params.get(key).and_then(|value| value.parse().ok())
}

fn parse_usize(request: &ControlRequest, key: &str) -> Option<usize> {
    request.params.get(key).and_then(|value| value.parse().ok())
}

fn select_workspace(terminal: &Rc<RefCell<Terminal>>, id: &str) -> bool {
    let mut terminal = terminal.borrow_mut();
    let Some(navigation) = terminal.navigation.as_mut() else {
        return false;
    };
    if id == "current" {
        return true;
    }
    let index = id.parse::<usize>().ok().or_else(|| {
        navigation
            .workspaces()
            .iter()
            .position(|item| item.id().to_string() == id)
    });
    index.is_some_and(|index| navigation.select_workspace(index))
}

fn select_group(terminal: &Rc<RefCell<Terminal>>, id: &str) -> bool {
    let mut terminal = terminal.borrow_mut();
    let Some(navigation) = terminal.navigation.as_mut() else {
        return false;
    };
    if id == "current" {
        return true;
    }
    let index = id.parse::<usize>().ok().or_else(|| {
        navigation
            .active_workspace()
            .groups()
            .iter()
            .position(|item| item.id().to_string() == id)
    });
    index.is_some_and(|index| navigation.active_workspace_mut().select_group(index))
}

fn select_tab(terminal: &Rc<RefCell<Terminal>>, id: &str) -> bool {
    let mut terminal = terminal.borrow_mut();
    let Some(navigation) = terminal.navigation.as_mut() else {
        return false;
    };
    if id == "current" {
        return true;
    }
    let index = id.parse::<usize>().ok().or_else(|| {
        navigation
            .active_workspace()
            .active_group()
            .tabs()
            .iter()
            .position(|item| item.id().to_string() == id)
    });
    index.is_some_and(|index| {
        navigation
            .active_workspace_mut()
            .active_group_mut()
            .select_tab(index)
    })
}

fn select_pane(terminal: &Rc<RefCell<Terminal>>, id: &str) -> bool {
    if id == "current" {
        return true;
    }
    let Ok(id) = PaneId::from_str(id) else {
        return false;
    };
    terminal
        .borrow_mut()
        .navigation
        .as_mut()
        .is_some_and(|navigation| navigation.focus_pane(id))
}

fn navigation_selection(terminal: &Rc<RefCell<Terminal>>) -> Option<(usize, usize, usize)> {
    let terminal = terminal.borrow();
    let navigation = terminal.navigation.as_ref()?;
    Some((
        navigation.active_workspace_index(),
        navigation.active_workspace().active_group_index(),
        navigation
            .active_workspace()
            .active_group()
            .active_tab_index(),
    ))
}

fn restore_navigation(terminal: &Rc<RefCell<Terminal>>, selection: Option<(usize, usize, usize)>) {
    let Some((workspace, group, tab)) = selection else {
        return;
    };
    let mut terminal = terminal.borrow_mut();
    let Some(navigation) = terminal.navigation.as_mut() else {
        return;
    };
    if navigation.select_workspace(workspace)
        && navigation.active_workspace_mut().select_group(group)
    {
        let _ = navigation
            .active_workspace_mut()
            .active_group_mut()
            .select_tab(tab);
    }
}

fn control_target_exists(terminal: &Rc<RefCell<Terminal>>, noun: &str, id: &str) -> bool {
    let terminal = terminal.borrow();
    let Some(navigation) = terminal.navigation.as_ref() else {
        return false;
    };
    if id == "current" {
        return true;
    }
    match noun {
        "workspace" => {
            id.parse::<usize>()
                .is_ok_and(|index| index < navigation.workspaces().len())
                || navigation
                    .workspaces()
                    .iter()
                    .any(|workspace| workspace.id().to_string() == id)
        }
        "group" => {
            id.parse::<usize>()
                .is_ok_and(|index| index < navigation.active_workspace().groups().len())
                || navigation
                    .active_workspace()
                    .groups()
                    .iter()
                    .any(|group| group.id().to_string() == id)
        }
        "tab" => {
            id.parse::<usize>().is_ok_and(|index| {
                index < navigation.active_workspace().active_group().tabs().len()
            }) || navigation
                .active_workspace()
                .active_group()
                .tabs()
                .iter()
                .any(|tab| tab.id().to_string() == id)
        }
        "pane" => PaneId::from_str(id).is_ok_and(|pane_id| {
            navigation
                .runtime_presentations()
                .iter()
                .any(|presentation| presentation.location.pane_id == pane_id)
        }),
        _ => false,
    }
}

fn resolve_pane_surface(terminal: &Rc<RefCell<Terminal>>, id: &str) -> Option<SurfaceId> {
    let terminal = terminal.borrow();
    if id == "current" {
        return terminal
            .sessions
            .contains_key(&terminal.active_surface_id)
            .then_some(terminal.active_surface_id);
    }
    let pane_id = PaneId::from_str(id).ok()?;
    let navigation = terminal.navigation.as_ref()?;
    navigation
        .runtime_presentations()
        .into_iter()
        .find_map(|presentation| {
            (presentation.location.pane_id == pane_id
                && terminal
                    .sessions
                    .contains_key(&presentation.location.surface_id))
            .then_some(presentation.location.surface_id)
        })
}

fn active_surface_matches_pane(terminal: &Rc<RefCell<Terminal>>, id: &str) -> bool {
    let active_surface = terminal.borrow().active_surface_id;
    resolve_pane_surface(terminal, id) == Some(active_surface)
}

fn split_geometry(area: &GLArea) -> (PixelRect, i32, PixelSize) {
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

fn rect_contains(rect: PixelRect, x: f64, y: f64) -> bool {
    x >= f64::from(rect.x)
        && x < f64::from(rect.x + rect.width)
        && y >= f64::from(rect.y)
        && y < f64::from(rect.y + rect.height)
}

fn pending_pane(id: PaneId, surface_id: SurfaceId) -> PaneContainer {
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

fn disconnected_ssh_argv(profile_id: Uuid) -> Vec<OsString> {
    vec![
        OsString::from("/usr/bin/printf"),
        OsString::from("%s\\n"),
        OsString::from(format!(
            "SSH profile {profile_id} restored disconnected; use explicit reconnect."
        )),
    ]
}

fn pending_tab() -> (TabModel, SurfaceId) {
    let pane = PaneId::new();
    let surface = SurfaceId::new();
    (
        TabModel::single(TabId::new(), pending_pane(pane, surface)),
        surface,
    )
}

fn pending_group() -> (GroupModel, SurfaceId) {
    let (tab, surface) = pending_tab();
    (GroupModel::single(GroupId::new(), tab), surface)
}

fn pending_workspace() -> (WorkspaceModel, SurfaceId) {
    let (group, surface) = pending_group();
    (WorkspaceModel::single(WorkspaceId::new(), group), surface)
}

fn initial_navigation(workspace: WorkspaceId, pane: PaneId, surface: SurfaceId) -> AppModel {
    AppModel::single(WorkspaceModel::single(
        workspace,
        GroupModel::single(
            GroupId::new(),
            TabModel::single(TabId::new(), pending_pane(pane, surface)),
        ),
    ))
}

impl Terminal {
    fn initialize(
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
                Vec::new(),
            )
        };
        self.pending_resume_offers = resume_offers;
        let restored_resume_command = surface_resume_commands
            .get(&self.active_surface_id)
            .cloned();
        let restored_ssh_profile = surface_ssh_profiles.get(&self.active_surface_id).copied();
        let restored_cwd = surface_cwds.get(&self.active_surface_id).cloned();
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

    fn split_layout(&self, area: &GLArea) -> Option<SplitLayout> {
        let (rect, gap, minimum) = split_geometry(area);
        Some(
            self.navigation
                .as_ref()?
                .active_tab()
                .layout(rect, gap, minimum),
        )
    }

    fn focus_pane_at(&mut self, area: &GLArea, x: f64, y: f64) -> bool {
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

    fn divider_at(&self, area: &GLArea, x: f64, y: f64) -> Option<SplitId> {
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

    fn resize_divider(&mut self, area: &GLArea, split_id: SplitId, x: f64, y: f64) -> bool {
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

    fn render(&mut self, area: &GLArea, status: &Label) {
        if self.engine.is_null() {
            return;
        }
        let factor = area.scale_factor().max(1);
        let width = area.width().max(1) * factor;
        let height = area.height().max(1) * factor;
        let Some(navigation) = self.navigation.as_ref() else {
            return;
        };
        let layout = navigation.active_tab().layout(
            PixelRect::new(0, 0, width, height),
            SPLIT_GAP * factor,
            MINIMUM_PANE,
        );
        let visible = navigation
            .runtime_presentations()
            .into_iter()
            .filter(|presentation| presentation.surface_visible)
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

    fn route_key(&mut self, input: &KitmuxGdkKeyInput, committed_text: Option<&str>) {
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

    fn shortcut(&self, keyval: gdk::Key, state: gdk::ModifierType) -> Option<ShortcutAction> {
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

    fn spawn_surface(&mut self, surface_id: SurfaceId) -> bool {
        let cwd = self.account_home.clone();
        self.spawn_surface_at(surface_id, &cwd)
    }

    fn spawn_surface_at(&mut self, surface_id: SurfaceId, cwd: &Path) -> bool {
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

    fn spawn_restored_surface(
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

    fn spawn_surface_command(
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

    fn create_session_state(
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

    fn replace_ssh_surface(
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

    fn create_ssh_tab(&mut self, profile: &SshProfile, executable: &Path) -> bool {
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

    fn navigation_action(&mut self, command: CommandId) -> NavigationEffect {
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

    fn select_navigation_target(&mut self, target: NavigationTarget) -> NavigationEffect {
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

    fn rename_navigation(&mut self, target: RenameTarget, name: &str) -> bool {
        let Some(navigation) = self.navigation.as_mut() else {
            return false;
        };
        match target {
            RenameTarget::Workspace(id) => navigation.rename_workspace(id, name),
            RenameTarget::Group(id) => navigation.rename_group(id, name),
            RenameTarget::Tab(id) => navigation.rename_tab(id, Some(name)),
        }
    }

    fn move_active_workspace(&mut self, direction: isize) -> bool {
        let Some(navigation) = self.navigation.as_mut() else {
            return false;
        };
        let index = navigation.active_workspace_index();
        let target = index.saturating_add_signed(direction);
        let id = navigation.active_workspace().id();
        navigation.move_workspace(id, target)
    }

    fn move_active_tab(&mut self, direction: isize) -> bool {
        let Some(navigation) = self.navigation.as_mut() else {
            return false;
        };
        let group = navigation.active_workspace_mut().active_group_mut();
        let index = group.active_tab_index();
        let target = index.saturating_add_signed(direction);
        let id = group.active_tab().id();
        group.move_tab(id, target)
    }

    fn clear_selection(&mut self) {
        if !self.session.is_null() {
            unsafe { ffi::kitty_session_selection_clear(self.session) };
        }
        self.selection_active = false;
    }

    fn selection_text(&self) -> Option<String> {
        if self.session.is_null() {
            return None;
        }
        owned_c_string(unsafe { ffi::kitty_session_selection_text(self.session) })
            .filter(|text| !text.is_empty())
    }

    fn paste(&mut self, text: &str) {
        if self.session.is_null() || text.is_empty() {
            return;
        }
        self.clear_selection();
        unsafe { ffi::kitty_session_paste(self.session, text.as_ptr(), text.len()) };
        diagnostic("paste", &[("bytes", text.len().to_string())]);
    }

    fn set_font_size(&mut self, area: &GLArea, points: f64) {
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

    fn change_font_size(&mut self, area: &GLArea, delta: f64) {
        if self.engine.is_null() {
            return;
        }
        let current = unsafe { ffi::kitty_render_font_size(self.engine) };
        self.set_font_size(area, current + delta);
    }

    fn search(&mut self, query: &str) -> Result<usize, String> {
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

    fn navigate_search(&mut self, backwards: bool) -> bool {
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

    fn cell_at(
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

    fn send_mouse(
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

    fn start_selection(&mut self, area: &GLArea, x: f64, y: f64, press_count: c_int) {
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

    fn update_selection(&mut self, area: &GLArea, x: f64, y: f64, ended: bool) {
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

    fn url_at(&self, area: &GLArea, x: f64, y: f64) -> Option<String> {
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

    fn foreground_surfaces(&self, scope: Option<ForegroundScope>) -> Vec<SurfaceId> {
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

    fn im_commit(&mut self, text: &str) {
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

    fn refresh_cwd(&mut self) {
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

    fn poll_settings(&mut self) {
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

    fn apply_settings(&mut self, document: SettingsDocument) {
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

    fn snapshot(&self) -> AppSnapshot {
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

    fn persist_state_now(&self) -> Result<(), String> {
        let Some(persistence) = self.persistence.as_ref() else {
            return Err("persistence unavailable".to_owned());
        };
        if !persistence.state_may_write {
            return Err("state input was not safely writable".to_owned());
        }
        save_state(&persistence.state_path, self.snapshot())
    }

    fn resume_current_state(&self, surface_id: SurfaceId) -> Option<ResumeCommandCurrentState> {
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

    fn run_resume_commands(&mut self, policy: ResumeCommandSelectionPolicy) {
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

    fn shutdown(&mut self, area: &GLArea) {
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

fn owned_c_string(value: *mut c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned();
    unsafe { libc::free(value.cast()) };
    Some(text)
}

fn c_buffer(value: &[c_char]) -> Option<String> {
    let end = value.iter().position(|byte| *byte == 0)?;
    let bytes = &value[..end];
    Some(
        String::from_utf8_lossy(unsafe {
            std::slice::from_raw_parts(bytes.as_ptr().cast::<u8>(), bytes.len())
        })
        .into_owned(),
    )
}

fn kitty_stage_error(stage: &str, error: &[c_char]) -> String {
    let detail = c_buffer(error)
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| "libkitty returned no diagnostic".to_owned());
    format!("{stage}: {detail}")
}

struct PumpContext {
    terminal: Weak<RefCell<Terminal>>,
    surface_id: SurfaceId,
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

fn attach_pty_source(
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

fn attach_missing_pty_sources(terminal: &Rc<RefCell<Terminal>>) -> Result<(), &'static str> {
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

fn attach_settings_source(terminal: &Rc<RefCell<Terminal>>) {
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

fn attach_sigterm_source(
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

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn refresh_navigation(terminal: &Rc<RefCell<Terminal>>) {
    let (ui, workspaces, tabs, active_workspace, active_tab, group_name, title) = {
        let terminal = terminal.borrow();
        let Some(ui) = terminal.navigation_ui.as_ref() else {
            return;
        };
        let Some(navigation) = terminal.navigation.as_ref() else {
            return;
        };
        let workspace = navigation.active_workspace();
        let group = workspace.active_group();
        let workspaces = navigation
            .workspaces()
            .iter()
            .map(|workspace| (workspace.id(), workspace.name().to_owned()))
            .collect::<Vec<_>>();
        let tabs = group
            .tabs()
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                (
                    tab.id(),
                    tab.custom_title()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("Tab {}", index + 1)),
                )
            })
            .collect::<Vec<_>>();
        let tab_name = group.active_tab().custom_title().map_or_else(
            || format!("Tab {}", group.active_tab_index() + 1),
            str::to_owned,
        );
        (
            ui.clone(),
            workspaces,
            tabs,
            navigation.active_workspace_index(),
            group.active_tab_index(),
            group.name().to_owned(),
            format!("{} ▸ {} ▸ {tab_name}", workspace.name(), group.name()),
        )
    };
    let (Some(sidebar), Some(tab_strip), Some(group_label)) = (
        ui.sidebar.upgrade(),
        ui.tab_strip.upgrade(),
        ui.group_label.upgrade(),
    ) else {
        return;
    };

    clear_box(&sidebar);
    for (index, (_, name)) in workspaces.into_iter().enumerate() {
        let button = Button::with_label(&format!("{}  {name}", index + 1));
        button.set_hexpand(true);
        button.set_focus_on_click(false);
        button.update_property(&[gtk::accessible::Property::Label(&format!(
            "Select workspace {name}"
        ))]);
        if index == active_workspace {
            button.add_css_class("suggested-action");
        }
        let weak = Rc::downgrade(terminal);
        button.connect_clicked(move |_| {
            let Some(terminal) = weak.upgrade() else {
                return;
            };
            let effect = terminal
                .borrow_mut()
                .select_navigation_target(NavigationTarget::Workspace(index));
            apply_navigation_effect(&terminal, effect);
        });
        sidebar.append(&button);
    }

    clear_box(&tab_strip);
    for (index, (_, name)) in tabs.into_iter().enumerate() {
        let button = Button::with_label(&name);
        button.set_focus_on_click(false);
        button.update_property(&[gtk::accessible::Property::Label(&format!(
            "Select terminal tab {name}"
        ))]);
        if index == active_tab {
            button.add_css_class("suggested-action");
        }
        let weak = Rc::downgrade(terminal);
        button.connect_clicked(move |_| {
            let Some(terminal) = weak.upgrade() else {
                return;
            };
            let effect = terminal
                .borrow_mut()
                .select_navigation_target(NavigationTarget::TerminalTab(index));
            apply_navigation_effect(&terminal, effect);
        });
        tab_strip.append(&button);
    }
    group_label.set_text(&group_name);
    if let Some(window) = ui.window.upgrade() {
        window.set_title(Some(&title));
    }
    if let Some(area) = ui.area.upgrade() {
        area.grab_focus();
    }
}

fn reconcile_sessions(terminal: &Rc<RefCell<Terminal>>) {
    let (expected, active_surface, area) = {
        let terminal = terminal.borrow();
        let Some(navigation) = terminal.navigation.as_ref() else {
            return;
        };
        let presentations = navigation.runtime_presentations();
        (
            presentations
                .iter()
                .map(|presentation| presentation.location.surface_id)
                .collect::<HashSet<_>>(),
            presentations
                .iter()
                .find(|presentation| presentation.accepts_input)
                .map(|presentation| presentation.location.surface_id),
            terminal
                .navigation_ui
                .as_ref()
                .and_then(|ui| ui.area.upgrade()),
        )
    };
    let Some(active_surface) = active_surface else {
        return;
    };
    let removed = {
        let terminal = terminal.borrow();
        terminal
            .sessions
            .keys()
            .filter(|surface| !expected.contains(surface))
            .copied()
            .collect::<Vec<_>>()
    };
    let mut terminal_mut = terminal.borrow_mut();
    if !terminal_mut.sessions.contains_key(&active_surface) {
        return;
    }
    for surface in removed {
        if let Some(mut session) = terminal_mut.sessions.remove(&surface) {
            if session.pty_source != 0 {
                unsafe { g_source_remove(session.pty_source) };
            }
            if !session.session.is_null() {
                unsafe { ffi::kitty_session_close(session.session) };
                session.session = ptr::null_mut();
            }
        }
    }
    terminal_mut.active_surface_id = active_surface;
    for (surface, session) in &mut terminal_mut.sessions {
        if let Some(callback) = &session.callback_ui {
            callback.visible.set(*surface == active_surface);
        }
    }
    drop(terminal_mut);
    let _ = attach_missing_pty_sources(terminal);
    if let Some(area) = area {
        area.queue_render();
        area.grab_focus();
    }
}

fn apply_navigation_effect(terminal: &Rc<RefCell<Terminal>>, effect: NavigationEffect) {
    let (window, area) = terminal
        .borrow()
        .navigation_ui
        .as_ref()
        .map(|ui| (ui.window.upgrade(), ui.area.upgrade()))
        .unwrap_or((None, None));
    match effect {
        NavigationEffect::Changed => {
            reconcile_sessions(terminal);
            let (workspaces, groups, tabs, workspace, group, tab, panes, focused) = {
                let terminal = terminal.borrow();
                let Some(navigation) = terminal.navigation.as_ref() else {
                    diagnostic("navigation_not_ready", &[]);
                    return;
                };
                let workspace_model = navigation.active_workspace();
                let group_model = workspace_model.active_group();
                (
                    navigation.workspaces().len(),
                    workspace_model.groups().len(),
                    group_model.tabs().len(),
                    navigation.active_workspace_index(),
                    workspace_model.active_group_index(),
                    group_model.active_tab_index(),
                    group_model.active_tab().pane_count(),
                    group_model.active_tab().focused_pane_id(),
                )
            };
            diagnostic(
                "navigation_changed",
                &[
                    ("workspaces", workspaces.to_string()),
                    ("groups", groups.to_string()),
                    ("tabs", tabs.to_string()),
                    ("workspace", workspace.to_string()),
                    ("group", group.to_string()),
                    ("tab", tab.to_string()),
                ],
            );
            if panes > 1 {
                diagnostic(
                    "split_changed",
                    &[
                        ("panes", panes.to_string()),
                        ("focused", focused.to_string()),
                    ],
                );
            }
            refresh_navigation(terminal);
            if let Some(area) = area {
                area.grab_focus();
            }
        }
        NavigationEffect::Rejected => {
            diagnostic("navigation_rejected", &[]);
            if let Some(area) = area {
                area.error_bell();
            }
        }
        NavigationEffect::CloseWindow => {
            if let Some(window) = window {
                window.close();
            }
        }
        NavigationEffect::Rename(target) => {
            if let (Some(window), Some(area)) = (window, area) {
                request_navigation_rename(terminal, target, &window, &area);
            }
        }
    }
}

fn run_navigation_gate_driver(terminal: &Rc<RefCell<Terminal>>) {
    // Test-only driver for native Wayland, where the nested X11 compositor
    // cannot safely forward host Super shortcuts. X11 exercises the real keys.
    let navigate = |command| {
        let effect = terminal.borrow_mut().navigation_action(command);
        apply_navigation_effect(terminal, effect);
    };
    let select = |target| {
        let effect = terminal.borrow_mut().select_navigation_target(target);
        apply_navigation_effect(terminal, effect);
    };
    if env::var_os("KITMUX_RAPID_NAV_GATE").is_some() {
        for _ in 0..8 {
            navigate(CommandId::TerminalNewTab);
        }
        for _ in 0..10 {
            for index in 0..9 {
                select(NavigationTarget::TerminalTab(index));
            }
        }
        for _ in 0..8 {
            navigate(CommandId::WorkspaceNew);
        }
        for _ in 0..10 {
            for index in 0..9 {
                select(NavigationTarget::Workspace(index));
            }
        }
        return;
    }
    if env::var_os("KITMUX_HIDDEN_SESSION_GATE").is_some() {
        navigate(CommandId::TerminalNewTab);
        let weak = Rc::downgrade(terminal);
        glib::timeout_add_local_once(Duration::from_millis(500), move || {
            let Some(terminal) = weak.upgrade() else {
                return;
            };
            let effect = terminal
                .borrow_mut()
                .select_navigation_target(NavigationTarget::TerminalTab(0));
            apply_navigation_effect(&terminal, effect);
        });
        return;
    }
    if env::var_os("KITMUX_SPLIT_GATE").is_some() {
        navigate(CommandId::PaneSplitRight);
        navigate(CommandId::PaneSplitDown);
        navigate(CommandId::PaneFocusPrevious);
        navigate(CommandId::PaneResizeLeft);
        return;
    }
    navigate(CommandId::WorkspaceNew);
    select(NavigationTarget::Workspace(0));
    navigate(CommandId::TerminalNewTab);
    select(NavigationTarget::TerminalTab(0));
    navigate(CommandId::GroupNew);
    navigate(CommandId::GroupPrevious);
    navigate(CommandId::GroupNext);
}

fn run_accessibility_gate(terminal: &Rc<RefCell<Terminal>>) {
    let Some(ui) = terminal.borrow().navigation_ui.clone() else {
        return;
    };
    let (Some(area), Some(commands), Some(settings)) = (
        ui.area.upgrade(),
        ui.command_palette.upgrade(),
        ui.settings.upgrade(),
    ) else {
        return;
    };
    let roles = gtk::test_accessible_has_role(&area, gtk::AccessibleRole::Terminal)
        && gtk::test_accessible_has_role(&commands, gtk::AccessibleRole::Button)
        && gtk::test_accessible_has_role(&settings, gtk::AccessibleRole::Button);
    let terminal_focused = area.grab_focus();
    let commands_focused = commands.grab_focus();
    let settings_focused = settings.grab_focus();
    let returned = area.grab_focus();
    diagnostic(
        if roles && terminal_focused && commands_focused && settings_focused && returned {
            "accessibility_ready"
        } else {
            "accessibility_failed"
        },
        &[
            ("roles", roles.to_string()),
            (
                "focus",
                (terminal_focused && commands_focused && settings_focused && returned).to_string(),
            ),
        ],
    );
}

fn open_modal_dialog(terminal: &Rc<RefCell<Terminal>>) -> bool {
    let mut terminal = terminal.borrow_mut();
    if terminal.modal_dialog_open {
        return false;
    }
    terminal.modal_dialog_open = true;
    true
}

fn request_navigation_rename(
    terminal: &Rc<RefCell<Terminal>>,
    target: RenameTarget,
    window: &ApplicationWindow,
    area: &GLArea,
) {
    if !open_modal_dialog(terminal) {
        return;
    }
    let dialog = gtk::Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Rename navigation item")
        .default_width(320)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    let entry = Entry::builder()
        .placeholder_text("Name")
        .max_length(256)
        .build();
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let cancel = Button::with_label("Cancel");
    let rename = Button::with_label("Rename");
    actions.append(&cancel);
    actions.append(&rename);
    content.append(&entry);
    content.append(&actions);
    dialog.set_child(Some(&content));

    let dialog_terminal = terminal.clone();
    let dialog_area = area.clone();
    dialog.connect_close_request(move |_| {
        dialog_terminal.borrow_mut().modal_dialog_open = false;
        dialog_area.grab_focus();
        Propagation::Proceed
    });

    let dialog_cancel = dialog.downgrade();
    let cancel_area = area.clone();
    cancel.connect_clicked(move |_| {
        if let Some(dialog) = dialog_cancel.upgrade() {
            dialog.close();
        }
        cancel_area.grab_focus();
    });

    let weak = Rc::downgrade(terminal);
    let dialog_rename = dialog.downgrade();
    let area = area.clone();
    let rename_entry = entry.clone();
    rename.connect_clicked(move |_| {
        if let Some(terminal) = weak.upgrade() {
            if terminal
                .borrow_mut()
                .rename_navigation(target, &rename_entry.text())
            {
                diagnostic("navigation_renamed", &[]);
                refresh_navigation(&terminal);
            } else {
                area.error_bell();
            }
        }
        if let Some(dialog) = dialog_rename.upgrade() {
            dialog.close();
        }
        area.grab_focus();
    });
    dialog.present();
    entry.grab_focus();
}

fn palette_command_supported(command: CommandId) -> bool {
    !matches!(
        command,
        CommandId::BrowserNewPane
            | CommandId::NotificationJumpUnread
            | CommandId::AppInstallCommandLineTool
            | CommandId::AppReloadKittyConfig
    )
}

fn apply_navigation_command(terminal: &Rc<RefCell<Terminal>>, command: CommandId, reviewed: bool) {
    let effect = terminal.borrow_mut().navigation_action(command);
    if reviewed && matches!(effect, NavigationEffect::CloseWindow) {
        terminal.borrow_mut().close_confirmed = true;
    }
    apply_navigation_effect(terminal, effect);
}

fn request_navigation_command(
    terminal: &Rc<RefCell<Terminal>>,
    command: CommandId,
    window: &ApplicationWindow,
    area: &GLArea,
) {
    if command == CommandId::TerminalResumeCommand {
        request_resume_command(terminal, window, area);
        return;
    }
    if !matches!(
        command,
        CommandId::PaneClose | CommandId::GroupClose | CommandId::WorkspaceClose
    ) {
        apply_navigation_command(terminal, command, false);
        return;
    }
    let foreground = {
        let terminal = terminal.borrow();
        if !terminal.confirm_close_with_running_process {
            Vec::new()
        } else {
            terminal.foreground_surfaces(Some(foreground_scope(command)))
        }
    };
    if foreground.is_empty() {
        apply_navigation_command(terminal, command, false);
        return;
    }
    if terminal.borrow().modal_dialog_open {
        return;
    }
    if let Some(confirm) = autoclose_decision() {
        if confirm {
            diagnostic(
                "close_scope_reviewed",
                &[
                    ("command", command.as_str().to_owned()),
                    ("sessions", foreground.len().to_string()),
                ],
            );
            apply_navigation_command(terminal, command, true);
        } else {
            diagnostic("close_cancelled", &[]);
        }
        return;
    }
    terminal.borrow_mut().modal_dialog_open = true;
    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message(format!(
            "Close {} terminal session{} with running processes?",
            foreground.len(),
            if foreground.len() == 1 { "" } else { "s" }
        ))
        .detail("Only the selected pane, group, or workspace will close.")
        .buttons(["Cancel", "Close"])
        .cancel_button(0)
        .default_button(0)
        .build();
    let terminal_confirm = terminal.clone();
    let area_confirm = area.clone();
    dialog.choose(Some(window), None::<&gio::Cancellable>, move |choice| {
        terminal_confirm.borrow_mut().modal_dialog_open = false;
        if matches!(choice, Ok(1)) {
            let rechecked = terminal_confirm
                .borrow()
                .foreground_surfaces(Some(foreground_scope(command)))
                .len();
            diagnostic(
                "close_scope_reviewed",
                &[
                    ("command", command.as_str().to_owned()),
                    ("sessions", rechecked.to_string()),
                ],
            );
            apply_navigation_command(&terminal_confirm, command, true);
        } else {
            area_confirm.grab_focus();
            diagnostic("close_cancelled", &[]);
        }
    });
}

fn request_resume_command(
    terminal: &Rc<RefCell<Terminal>>,
    window: &ApplicationWindow,
    area: &GLArea,
) {
    if !open_modal_dialog(terminal) {
        return;
    }
    let current = terminal.borrow().resume_command.clone().unwrap_or_default();
    let dialog = gtk::Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Set startup / resume command")
        .default_width(560)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    let note = Label::new(Some(
        "The command is saved as inert text and always needs explicit review before it runs.",
    ));
    note.set_wrap(true);
    note.set_xalign(0.0);
    let entry = Entry::builder().text(&current).build();
    entry.update_property(&[gtk::accessible::Property::Label("Startup / resume command")]);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let cancel = Button::with_label("Cancel");
    let save = Button::with_label("Save");
    actions.append(&cancel);
    actions.append(&save);
    content.append(&note);
    content.append(&entry);
    content.append(&actions);
    dialog.set_child(Some(&content));

    let dialog_cancel = dialog.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(dialog) = dialog_cancel.upgrade() {
            dialog.close();
        }
    });
    let terminal_save = terminal.clone();
    let dialog_save = dialog.downgrade();
    let area_save = area.clone();
    let entry_save = entry.clone();
    save.connect_clicked(move |_| {
        let raw = entry_save.text();
        let command = if raw.trim().is_empty() {
            None
        } else {
            let Some(command) = valid_resume_command(Some(raw.as_str())) else {
                area_save.error_bell();
                diagnostic("resume_command_rejected", &[]);
                return;
            };
            Some(command)
        };
        let mut terminal = terminal_save.borrow_mut();
        terminal.resume_command = command;
        if terminal.persist_state_now().is_err() {
            diagnostic(
                "state_save_failed",
                &[("reason", "resume-command".to_owned())],
            );
        }
        drop(terminal);
        if let Some(dialog) = dialog_save.upgrade() {
            dialog.close();
        }
        diagnostic("resume_command_saved", &[]);
    });
    let terminal_close = terminal.clone();
    let area_close = area.clone();
    dialog.connect_close_request(move |_| {
        terminal_close.borrow_mut().modal_dialog_open = false;
        area_close.grab_focus();
        Propagation::Proceed
    });
    dialog.present();
    entry.grab_focus();
}

fn present_resume_offers(
    terminal: &Rc<RefCell<Terminal>>,
    window: &ApplicationWindow,
    area: &GLArea,
) {
    let offers = {
        let mut terminal_state = terminal.borrow_mut();
        std::mem::take(&mut terminal_state.pending_resume_offers)
    };
    if offers.is_empty() || !open_modal_dialog(terminal) {
        return;
    }
    let dialog = gtk::Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Review saved commands")
        .default_width(560)
        .default_height(420)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    let explanation = Label::new(Some(
        "Saved commands are shown for review. Nothing runs unless you select it.",
    ));
    explanation.set_wrap(true);
    explanation.set_xalign(0.0);
    content.append(&explanation);
    let rows_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&rows_box)
        .build();
    content.append(&scroll);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let cancel = Button::with_label("Don't Run");
    let run = Button::with_label("Run selected");
    run.set_sensitive(false);
    actions.append(&cancel);
    actions.append(&run);
    content.append(&actions);
    dialog.set_child(Some(&content));

    let policy = Rc::new(RefCell::new(ResumeCommandSelectionPolicy::new(
        offers.iter().map(|offer| offer.identity.clone()).collect(),
    )));
    for offer in &offers {
        let row = gtk::Box::new(gtk::Orientation::Vertical, 3);
        let check = gtk::CheckButton::with_label(&offer.location);
        check.set_active(false);
        check.update_property(&[gtk::accessible::Property::Label(&format!(
            "Run saved command in {}",
            offer.location
        ))]);
        let command = Label::new(Some(&format!("Command: {}", offer.identity.command)));
        command.set_selectable(true);
        command.set_xalign(0.0);
        command.set_wrap(true);
        let cwd = Label::new(Some(&format!(
            "Working directory: {}",
            offer.identity.cwd.as_deref().unwrap_or("(default)")
        )));
        cwd.set_xalign(0.0);
        cwd.set_selectable(true);
        row.append(&check);
        row.append(&command);
        row.append(&cwd);
        rows_box.append(&row);

        let policy_changed = Rc::clone(&policy);
        let run_changed = run.clone();
        let surface_id = offer.identity.surface_id;
        check.connect_toggled(move |check| {
            let mut policy = policy_changed.borrow_mut();
            policy.set_selected(surface_id, check.is_active());
            run_changed.set_sensitive(!policy.selected_row_ids().is_empty());
        });
    }

    let terminal_cancel = terminal.clone();
    let dialog_cancel = dialog.downgrade();
    cancel.connect_clicked(move |_| {
        diagnostic("resume_review_cancelled", &[]);
        if let Some(dialog) = dialog_cancel.upgrade() {
            dialog.close();
        }
        terminal_cancel.borrow_mut().pending_resume_offers.clear();
    });
    let terminal_run = terminal.clone();
    let dialog_run = dialog.downgrade();
    let policy_run = Rc::clone(&policy);
    run.connect_clicked(move |_| {
        let policy = policy_run.borrow().clone();
        if policy.selected_row_ids().is_empty() {
            return;
        }
        diagnostic(
            "resume_review_approved",
            &[("selected", policy.selected_row_ids().len().to_string())],
        );
        if let Some(dialog) = dialog_run.upgrade() {
            dialog.close();
        }
        terminal_run.borrow_mut().run_resume_commands(policy);
    });

    let terminal_close = terminal.clone();
    let area_close = area.clone();
    dialog.connect_close_request(move |_| {
        terminal_close.borrow_mut().modal_dialog_open = false;
        area_close.grab_focus();
        Propagation::Proceed
    });
    dialog.present();
    diagnostic(
        "resume_review",
        &[
            ("rows", offers.len().to_string()),
            ("unchecked", "true".to_owned()),
        ],
    );

    match autoresume_decision() {
        Some("restore") => {
            if let Some(offer) = offers.first() {
                policy
                    .borrow_mut()
                    .set_selected(offer.identity.surface_id, true);
                let selected = policy.borrow().clone();
                dialog.close();
                terminal.borrow_mut().run_resume_commands(selected);
            }
        }
        Some("restore-all") => {
            let mut policy = policy.borrow_mut();
            for offer in &offers {
                policy.set_selected(offer.identity.surface_id, true);
            }
            let selected = policy.clone();
            drop(policy);
            dialog.close();
            terminal.borrow_mut().run_resume_commands(selected);
        }
        Some("race") => {
            if let Some(offer) = offers.first() {
                let surface = offer.identity.surface_id;
                if let Some(session) = terminal.borrow_mut().sessions.get_mut(&surface) {
                    session.resume_command = Some("printf resume-race-replacement".to_owned());
                }
                policy
                    .borrow_mut()
                    .set_selected(offer.identity.surface_id, true);
                let selected = policy.borrow().clone();
                dialog.close();
                terminal.borrow_mut().run_resume_commands(selected);
            }
        }
        Some("decline") => {
            diagnostic("resume_review_cancelled", &[]);
            dialog.close();
        }
        _ => {}
    }
}

fn execute_palette_command(
    command: CommandId,
    terminal: &Rc<RefCell<Terminal>>,
    window: &ApplicationWindow,
    area: &GLArea,
) {
    match command {
        CommandId::TerminalCopy => copy_selection(area, terminal),
        CommandId::TerminalPaste => request_paste(window, area, terminal),
        CommandId::TerminalFind => {
            if let Some(ui) = terminal.borrow().navigation_ui.as_ref()
                && let (Some(search_bar), Some(search_entry)) =
                    (ui.search_bar.upgrade(), ui.search_entry.upgrade())
            {
                search_bar.set_search_mode(true);
                search_entry.grab_focus();
            }
        }
        CommandId::TerminalClearScrollback => {
            let mut terminal = terminal.borrow_mut();
            terminal.clear_selection();
            if !terminal.session.is_null() {
                unsafe { ffi::kitty_session_clear_scrollback(terminal.session) };
                area.queue_render();
            }
        }
        CommandId::FontIncrease => terminal.borrow_mut().change_font_size(area, 2.0),
        CommandId::FontDecrease => terminal.borrow_mut().change_font_size(area, -2.0),
        CommandId::FontReset => {
            let size = terminal.borrow().default_font_size;
            terminal.borrow_mut().set_font_size(area, size);
        }
        CommandId::AppSettings => request_settings(window, area, terminal),
        CommandId::AppToggleSidebar => {
            let mut terminal = terminal.borrow_mut();
            let Some(sidebar) = terminal
                .navigation_ui
                .as_ref()
                .and_then(|ui| ui.sidebar_shell.upgrade())
            else {
                area.error_bell();
                return;
            };
            let visible = !sidebar.is_visible();
            sidebar.set_visible(visible);
            if let Some(persistence) = terminal.persistence.as_mut() {
                let mut document = persistence.settings.clone();
                let mut resolved = document.resolved().clone();
                resolved.sidebar_visible_on_launch = visible;
                resolved.sidebar_collapsed_on_launch = !visible;
                document.replace_resolved(resolved);
                if save_settings(&persistence.settings_path, &document).is_ok() {
                    persistence.settings = document;
                }
            }
        }
        _ => request_navigation_command(terminal, command, window, area),
    }
    diagnostic("palette_command", &[("id", command.as_str().to_owned())]);
}

fn populate_command_palette(
    commands: &gtk::Box,
    query: &str,
    terminal: &Rc<RefCell<Terminal>>,
    window: &ApplicationWindow,
    area: &GLArea,
    dialog: &gtk::Window,
) {
    clear_box(commands);
    for command in command_palette_matches(query) {
        let button = Button::with_label(command.as_str());
        button.set_sensitive(palette_command_supported(command));
        button.set_halign(gtk::Align::Fill);
        button.update_property(&[gtk::accessible::Property::Label(&format!(
            "Run command {}",
            command.as_str()
        ))]);
        let terminal = terminal.clone();
        let window = window.clone();
        let area = area.clone();
        let dialog = dialog.downgrade();
        button.connect_clicked(move |_| {
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
            execute_palette_command(command, &terminal, &window, &area);
        });
        commands.append(&button);
    }
}

fn request_command_palette(
    window: &ApplicationWindow,
    area: &GLArea,
    terminal: &Rc<RefCell<Terminal>>,
) {
    if !open_modal_dialog(terminal) {
        return;
    }
    let dialog = gtk::Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Command palette")
        .default_width(460)
        .default_height(420)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    let entry = Entry::builder().placeholder_text("Filter commands").build();
    entry.update_property(&[gtk::accessible::Property::Label("Filter commands")]);
    let commands = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&commands)
        .build();
    content.append(&entry);
    content.append(&scroll);
    dialog.set_child(Some(&content));
    populate_command_palette(&commands, "", terminal, window, area, &dialog);
    let terminal_changed = terminal.clone();
    let window_changed = window.clone();
    let area_changed = area.clone();
    let commands_changed = commands.clone();
    let dialog_changed = dialog.clone();
    entry.connect_changed(move |entry| {
        populate_command_palette(
            &commands_changed,
            entry.text().as_str(),
            &terminal_changed,
            &window_changed,
            &area_changed,
            &dialog_changed,
        );
    });
    let terminal_activate = terminal.clone();
    let window_activate = window.clone();
    let area_activate = area.clone();
    let dialog_activate = dialog.downgrade();
    entry.connect_activate(move |entry| {
        let Some(command) = command_palette_matches(entry.text().as_str())
            .into_iter()
            .find(|command| palette_command_supported(*command))
        else {
            area_activate.error_bell();
            return;
        };
        if let Some(dialog) = dialog_activate.upgrade() {
            dialog.close();
        }
        execute_palette_command(
            command,
            &terminal_activate,
            &window_activate,
            &area_activate,
        );
    });
    let palette_keys = gtk::EventControllerKey::new();
    let dialog_escape = dialog.downgrade();
    palette_keys.connect_key_pressed(move |_, key, _, _| {
        if key != gdk::Key::Escape {
            return Propagation::Proceed;
        }
        if let Some(dialog) = dialog_escape.upgrade() {
            dialog.close();
        }
        Propagation::Stop
    });
    dialog.add_controller(palette_keys);
    let dialog_terminal = terminal.clone();
    let area_close = area.clone();
    dialog.connect_close_request(move |_| {
        dialog_terminal.borrow_mut().modal_dialog_open = false;
        area_close.grab_focus();
        Propagation::Proceed
    });
    dialog.present();
    entry.grab_focus();
    diagnostic("command_palette_opened", &[]);
}

fn request_settings(window: &ApplicationWindow, area: &GLArea, terminal: &Rc<RefCell<Terminal>>) {
    let Some((settings_path, document)) = terminal
        .borrow()
        .persistence
        .as_ref()
        .map(|state| (state.settings_path.clone(), state.settings.clone()))
    else {
        area.error_bell();
        return;
    };
    if !open_modal_dialog(terminal) {
        return;
    }
    let resolved = document.resolved();
    let dialog = gtk::Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Kitmux settings")
        .default_width(420)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    let restore = gtk::CheckButton::with_label("Restore workspace layout on launch");
    restore.set_active(resolved.restore_layout == RestoreLayoutPolicy::Always);
    let sidebar = gtk::CheckButton::with_label("Show workspace sidebar on launch");
    sidebar.set_active(resolved.sidebar_visible_on_launch);
    let confirm = gtk::CheckButton::with_mnemonic("_Confirm before closing running processes");
    confirm.set_active(resolved.confirm_close_with_running_process);
    let paste_label = Label::new(Some("Paste confirmation threshold (bytes)"));
    paste_label.set_xalign(0.0);
    let paste = gtk::SpinButton::with_range(0.0, 10_485_760.0, 1024.0);
    paste.set_value(resolved.paste_confirmation_threshold_bytes as f64);
    let wheel_label = Label::new(Some("Mouse wheel lines"));
    wheel_label.set_xalign(0.0);
    let wheel = gtk::SpinButton::with_range(1.0, 10.0, 1.0);
    wheel.set_value(document.wheel_scroll_lines() as f64);
    let sidebar_width_label = Label::new(Some("Sidebar width (points)"));
    sidebar_width_label.set_xalign(0.0);
    let sidebar_width = gtk::SpinButton::with_range(140.0, 320.0, 1.0);
    sidebar_width.set_value(resolved.sidebar_width_points as f64);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let cancel = Button::with_label("Cancel");
    let save = Button::with_label("Save");
    actions.append(&cancel);
    actions.append(&save);
    for widget in [
        restore.upcast_ref::<gtk::Widget>(),
        sidebar.upcast_ref(),
        confirm.upcast_ref(),
        paste_label.upcast_ref(),
        paste.upcast_ref(),
        wheel_label.upcast_ref(),
        wheel.upcast_ref(),
        sidebar_width_label.upcast_ref(),
        sidebar_width.upcast_ref(),
        actions.upcast_ref(),
    ] {
        content.append(widget);
    }
    dialog.set_child(Some(&content));
    for (widget, name) in [
        (restore.upcast_ref::<gtk::Widget>(), "restore"),
        (sidebar.upcast_ref(), "sidebar"),
        (confirm.upcast_ref(), "confirm"),
        (paste.upcast_ref(), "paste-threshold"),
        (wheel.upcast_ref(), "wheel-lines"),
        (sidebar_width.upcast_ref(), "sidebar-width"),
        (cancel.upcast_ref(), "cancel"),
        (save.upcast_ref(), "save"),
    ] {
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(move |_| {
            diagnostic("settings_focus", &[("control", name.to_owned())]);
        });
        widget.add_controller(focus);
    }
    let dialog_cancel = dialog.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(dialog) = dialog_cancel.upgrade() {
            dialog.close();
        }
    });
    let terminal_save = terminal.clone();
    let dialog_save = dialog.downgrade();
    let restore_focus = restore.clone();
    let confirm_shortcut = confirm.clone();
    let dialog_document = document.clone();
    save.connect_clicked(move |_| {
        let current_document = terminal_save
            .borrow()
            .persistence
            .as_ref()
            .map(|state| state.settings.clone());
        if current_document.as_ref() != Some(&dialog_document) {
            if let Some(area) = terminal_save
                .borrow()
                .navigation_ui
                .as_ref()
                .and_then(|ui| ui.area.upgrade())
            {
                area.error_bell();
            }
            diagnostic("settings_changed_while_dialog_open", &[]);
            return;
        }
        let mut document = dialog_document.clone();
        let mut resolved = document.resolved().clone();
        resolved.restore_layout = if restore.is_active() {
            RestoreLayoutPolicy::Always
        } else {
            RestoreLayoutPolicy::Never
        };
        resolved.sidebar_visible_on_launch = sidebar.is_active();
        resolved.sidebar_collapsed_on_launch = !sidebar.is_active();
        resolved.confirm_close_with_running_process = confirm.is_active();
        resolved.paste_confirmation_threshold_bytes = paste.value() as u64;
        document.set_wheel_scroll_lines(wheel.value() as u64);
        resolved.sidebar_width_points = sidebar_width.value() as u64;
        document.replace_resolved(resolved);
        if save_settings(&settings_path, &document).is_ok() {
            terminal_save.borrow_mut().apply_settings(document);
            diagnostic("settings_saved", &[]);
            if let Some(dialog) = dialog_save.upgrade() {
                dialog.close();
            }
        } else if let Some(area) = terminal_save
            .borrow()
            .navigation_ui
            .as_ref()
            .and_then(|ui| ui.area.upgrade())
        {
            area.error_bell();
            diagnostic("settings_save_failed", &[]);
        }
    });
    let save_shortcut = save.clone();
    let settings_shortcuts = gtk::EventControllerKey::new();
    settings_shortcuts.set_propagation_phase(gtk::PropagationPhase::Capture);
    settings_shortcuts.connect_key_pressed(move |_, key, _, state| {
        if key
            .to_unicode()
            .is_some_and(|key| key.eq_ignore_ascii_case(&'c'))
            && state.contains(gdk::ModifierType::ALT_MASK)
        {
            confirm_shortcut.set_active(!confirm_shortcut.is_active());
            confirm_shortcut.grab_focus();
            return Propagation::Stop;
        }
        if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter)
            && state.contains(gdk::ModifierType::CONTROL_MASK)
        {
            save_shortcut.emit_clicked();
            return Propagation::Stop;
        }
        Propagation::Proceed
    });
    dialog.add_controller(settings_shortcuts);
    let dialog_terminal = terminal.clone();
    let area_close = area.clone();
    dialog.connect_close_request(move |_| {
        dialog_terminal.borrow_mut().modal_dialog_open = false;
        area_close.grab_focus();
        Propagation::Proceed
    });
    let settings_keys = gtk::EventControllerKey::new();
    let dialog_escape = dialog.downgrade();
    settings_keys.connect_key_pressed(move |_, key, _, _| {
        if key != gdk::Key::Escape {
            return Propagation::Proceed;
        }
        if let Some(dialog) = dialog_escape.upgrade() {
            dialog.close();
        }
        Propagation::Stop
    });
    dialog.add_controller(settings_keys);
    dialog.present();
    restore_focus.grab_focus();
    diagnostic("settings_opened", &[]);
}

fn copy_selection(area: &GLArea, terminal: &Rc<RefCell<Terminal>>) {
    let Some(text) = terminal.borrow().selection_text() else {
        area.error_bell();
        return;
    };
    area.clipboard().set_text(&text);
    diagnostic("clipboard_copy", &[("bytes", text.len().to_string())]);
}

fn request_paste(window: &ApplicationWindow, area: &GLArea, terminal: &Rc<RefCell<Terminal>>) {
    let window = window.clone();
    let area = area.clone();
    let terminal = terminal.clone();
    area.clipboard()
        .read_text_async(None::<&gio::Cancellable>, move |result| {
            let Ok(Some(text)) = result else {
                area.error_bell();
                return;
            };
            let text = text.to_string();
            let threshold = terminal.borrow().paste_confirmation_threshold;
            let Some(reason) = paste_confirmation_reason(&text, threshold) else {
                terminal.borrow_mut().paste(&text);
                area.queue_render();
                return;
            };
            match autopaste_decision() {
                Some(true) => {
                    terminal.borrow_mut().paste(&text);
                    area.queue_render();
                    return;
                }
                Some(false) => {
                    diagnostic("paste_cancelled", &[("reason", paste_reason(reason))]);
                    return;
                }
                None => {}
            }
            let (message, detail) = match reason {
                PasteConfirmationReason::Large { bytes } => (
                    format!("Paste {bytes} bytes?"),
                    "This large paste may run many commands at once.",
                ),
                PasteConfirmationReason::ControlCharacters => (
                    "Paste text with control characters?".to_owned(),
                    "Control characters can conceal terminal escape sequences.",
                ),
            };
            let dialog = gtk::AlertDialog::builder()
                .modal(true)
                .message(message)
                .detail(detail)
                .buttons(["Cancel", "Paste"])
                .cancel_button(0)
                .default_button(0)
                .build();
            dialog.choose(Some(&window), None::<&gio::Cancellable>, move |choice| {
                if choice == Ok(1) {
                    terminal.borrow_mut().paste(&text);
                    area.queue_render();
                } else {
                    diagnostic("paste_cancelled", &[("reason", paste_reason(reason))]);
                }
                area.grab_focus();
            });
        });
}

fn paste_reason(reason: PasteConfirmationReason) -> String {
    match reason {
        PasteConfirmationReason::Large { .. } => "large".to_owned(),
        PasteConfirmationReason::ControlCharacters => "controls".to_owned(),
    }
}

fn autoresume_decision() -> Option<&'static str> {
    if !cfg!(feature = "test-hooks") {
        return None;
    }
    match env::var("KITMUX_AUTORESUME").as_deref() {
        Ok("restore") => Some("restore"),
        Ok("restore-all") => Some("restore-all"),
        Ok("race") => Some("race"),
        Ok("decline") => Some("decline"),
        _ => None,
    }
}

fn autopaste_decision() -> Option<bool> {
    // Test-only driver for the modal path; ordinary launches leave it unset.
    // Compiled inert unless the `test-hooks` feature is on, so a release build
    // cannot have its unsafe-paste confirmation removed by the environment.
    if !cfg!(feature = "test-hooks") {
        return None;
    }
    match env::var("KITMUX_AUTOPASTE").as_deref() {
        Ok("confirm") => Some(true),
        Ok("cancel") => Some(false),
        Ok("cancel-first") => Some(UNSAFE_PASTE_COUNT.fetch_add(1, Ordering::Relaxed) > 0),
        _ => None,
    }
}

fn autoclose_decision() -> Option<bool> {
    // Test-only driver for both branches of the foreground-process prompt.
    // Compiled inert unless the `test-hooks` feature is on, so a release build
    // cannot have its running-process close confirmation removed by the environment.
    if !cfg!(feature = "test-hooks") {
        return None;
    }
    match env::var("KITMUX_AUTOCLOSE").as_deref() {
        Ok("confirm") => Some(true),
        Ok("cancel") => Some(false),
        Ok("cancel-first") => Some(FOREGROUND_CLOSE_COUNT.fetch_add(1, Ordering::Relaxed) > 0),
        _ => None,
    }
}

fn open_url(url: String) {
    gio::AppInfo::launch_default_for_uri_async(
        &url,
        None::<&gio::AppLaunchContext>,
        None::<&gio::Cancellable>,
        move |result| {
            diagnostic(
                "url_open",
                &[(
                    "result",
                    if result.is_ok() { "ok" } else { "error" }.to_owned(),
                )],
            );
        },
    );
}

fn main() -> glib::ExitCode {
    if !install_sigterm_handler() {
        diagnostic("sigterm_handler_failed", &[]);
    }
    let app = Application::builder()
        .application_id("dev.kitmux.Kitmux")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(window::build_window);
    app.run()
}
