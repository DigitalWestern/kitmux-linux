use gtk::gio;
use gtk::glib::{self};
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Button, Entry, GLArea, Label, SearchBar};
use kitmux_model::{CommandId, GroupId, NavigationTarget, TabId, WorkspaceId};
use std::cell::RefCell;
use std::collections::HashSet;
use std::env;
use std::ptr;
use std::rc::Rc;
use std::time::Duration;

use crate::diagnostic;
use crate::dialogs::request_navigation_rename;
use crate::ffi;
use crate::menu::rebuild_window_menu;
use crate::terminal::{Terminal, attach_missing_pty_sources, g_source_remove};

#[derive(Clone)]
pub(crate) struct NavigationUi {
    pub(crate) app: glib::WeakRef<Application>,
    pub(crate) sidebar_shell: glib::WeakRef<gtk::Box>,
    pub(crate) sidebar: glib::WeakRef<gtk::Box>,
    pub(crate) tab_strip: glib::WeakRef<gtk::Box>,
    pub(crate) group_label: glib::WeakRef<Label>,
    pub(crate) status: glib::WeakRef<Label>,
    pub(crate) window: glib::WeakRef<ApplicationWindow>,
    pub(crate) area: glib::WeakRef<GLArea>,
    pub(crate) search_bar: glib::WeakRef<SearchBar>,
    pub(crate) search_entry: glib::WeakRef<Entry>,
    pub(crate) menu_bar: glib::WeakRef<gtk::PopoverMenuBar>,
    pub(crate) workspace_menu: gio::Menu,
    pub(crate) tab_menu: gio::Menu,
    pub(crate) command_palette: glib::WeakRef<Button>,
    pub(crate) settings: glib::WeakRef<Button>,
}

#[derive(Clone, Copy)]
pub(crate) enum RenameTarget {
    Workspace(WorkspaceId),
    Group(GroupId),
    Tab(TabId),
}

pub(crate) enum NavigationEffect {
    Changed,
    Rejected,
    CloseWindow,
    Rename(RenameTarget),
}

#[derive(Clone, Copy)]
pub(crate) enum ForegroundScope {
    Pane,
    Tab,
    Group,
    Workspace,
}

pub(crate) fn changed(value: bool) -> NavigationEffect {
    if value {
        NavigationEffect::Changed
    } else {
        NavigationEffect::Rejected
    }
}

pub(crate) fn foreground_scope(command: CommandId) -> ForegroundScope {
    match command {
        CommandId::PaneClose => ForegroundScope::Pane,
        CommandId::TabClose => ForegroundScope::Tab,
        CommandId::GroupClose => ForegroundScope::Group,
        CommandId::WorkspaceClose => ForegroundScope::Workspace,
        _ => ForegroundScope::Pane,
    }
}

pub(crate) fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

pub(crate) fn refresh_navigation(terminal: &Rc<RefCell<Terminal>>) {
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

    rebuild_window_menu(&ui, &workspaces, &tabs);

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

pub(crate) fn reconcile_sessions(terminal: &Rc<RefCell<Terminal>>) {
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

pub(crate) fn apply_navigation_effect(terminal: &Rc<RefCell<Terminal>>, effect: NavigationEffect) {
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

pub(crate) fn run_navigation_gate_driver(terminal: &Rc<RefCell<Terminal>>) {
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

pub(crate) fn run_accessibility_gate(terminal: &Rc<RefCell<Terminal>>) {
    let Some(ui) = terminal.borrow().navigation_ui.clone() else {
        return;
    };
    let (Some(area), Some(commands), Some(settings), Some(menu_bar)) = (
        ui.area.upgrade(),
        ui.command_palette.upgrade(),
        ui.settings.upgrade(),
        ui.menu_bar.upgrade(),
    ) else {
        return;
    };
    let roles = gtk::test_accessible_has_role(&area, gtk::AccessibleRole::Terminal)
        && gtk::test_accessible_has_role(&commands, gtk::AccessibleRole::Button)
        && gtk::test_accessible_has_role(&settings, gtk::AccessibleRole::Button)
        && gtk::test_accessible_has_role(&menu_bar, gtk::AccessibleRole::MenuBar);
    let terminal_focused = area.grab_focus();
    let commands_focused = commands.grab_focus();
    let settings_focused = settings.grab_focus();
    let menu_focused = menu_bar.grab_focus();
    let returned = area.grab_focus();
    diagnostic(
        if roles
            && terminal_focused
            && commands_focused
            && settings_focused
            && menu_focused
            && returned
        {
            "accessibility_ready"
        } else {
            "accessibility_failed"
        },
        &[
            ("roles", roles.to_string()),
            (
                "focus",
                (terminal_focused
                    && commands_focused
                    && settings_focused
                    && menu_focused
                    && returned)
                    .to_string(),
            ),
        ],
    );
}
