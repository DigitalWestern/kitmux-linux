use gtk::gio;
use gtk::glib::{self};
use gtk::prelude::*;
use gtk::{AboutDialog, Application, ApplicationWindow, GLArea, License};
use kitmux_model::{CommandId, NavigationTarget, ShortcutAction, ShortcutMap, TabId, WorkspaceId};
use std::cell::RefCell;
use std::env;
use std::rc::Rc;

use crate::diagnostic;
use crate::dialogs::{execute_palette_command, open_url, request_command_palette};
use crate::navigation::{NavigationUi, apply_navigation_effect};
use crate::terminal::Terminal;

pub(crate) fn palette_command_supported(command: CommandId) -> bool {
    !matches!(
        command,
        CommandId::BrowserNewPane
            | CommandId::NotificationJumpUnread
            | CommandId::AppInstallCommandLineTool
            | CommandId::AppReloadKittyConfig
    )
}

pub(crate) struct MenuState {
    pub(crate) model: gio::Menu,
    pub(crate) workspace_menu: gio::Menu,
    pub(crate) tab_menu: gio::Menu,
}

pub(crate) fn command_action_name(command: CommandId) -> String {
    format!("command-{}", command.as_str().replace('.', "-"))
}

pub(crate) fn command_action(command: CommandId) -> String {
    format!("app.{}", command_action_name(command))
}

pub(crate) fn append_command(menu: &gio::Menu, label: &str, command: CommandId) {
    menu.append(Some(label), Some(&command_action(command)));
}

pub(crate) fn set_menu_accelerators(app: &Application, shortcuts: &ShortcutMap) {
    for command in CommandId::ALL {
        let detailed = command_action(*command);
        let accelerator = shortcuts.accelerator_for_command(*command);
        let accelerators = accelerator.as_deref().into_iter().collect::<Vec<_>>();
        app.set_accels_for_action(&detailed, &accelerators);
    }
    let accelerator = shortcuts.accelerator_for_action(ShortcutAction::CommandPalette);
    let accelerators = accelerator.as_deref().into_iter().collect::<Vec<_>>();
    app.set_accels_for_action("app.command-palette", &accelerators);
}

pub(crate) fn register_menu_action(
    app: &Application,
    terminal: &Rc<RefCell<Terminal>>,
    window: &ApplicationWindow,
    area: &GLArea,
    command: CommandId,
) {
    let action = gio::SimpleAction::new(&command_action_name(command), None);
    action.set_enabled(palette_command_supported(command));
    let weak = Rc::downgrade(terminal);
    let window = window.clone();
    let area = area.clone();
    action.connect_activate(move |_, _| {
        let Some(terminal) = weak.upgrade() else {
            return;
        };
        execute_palette_command(command, &terminal, &window, &area);
        diagnostic("menu_command", &[("id", command.as_str().to_owned())]);
    });
    app.add_action(&action);
}

pub(crate) fn register_menu_actions(
    app: &Application,
    terminal: &Rc<RefCell<Terminal>>,
    window: &ApplicationWindow,
    area: &GLArea,
) {
    for command in CommandId::ALL {
        register_menu_action(app, terminal, window, area, *command);
    }

    for name in ["new-window", "ssh-connect"] {
        let action = gio::SimpleAction::new(name, None);
        action.set_enabled(false);
        app.add_action(&action);
    }

    let command_palette = gio::SimpleAction::new("command-palette", None);
    let weak = Rc::downgrade(terminal);
    let window_palette = window.clone();
    let area_palette = area.clone();
    command_palette.connect_activate(move |_, _| {
        if let Some(terminal) = weak.upgrade() {
            request_command_palette(&window_palette, &area_palette, &terminal);
        }
    });
    app.add_action(&command_palette);

    let shortcuts = gio::SimpleAction::new("keyboard-shortcuts", None);
    let weak = Rc::downgrade(terminal);
    let window_shortcuts = window.clone();
    shortcuts.connect_activate(move |_, _| {
        if let Some(terminal) = weak.upgrade() {
            show_keyboard_shortcuts(&window_shortcuts, &terminal);
        }
    });
    app.add_action(&shortcuts);

    let workspace_select =
        gio::SimpleAction::new("select-workspace", Some(glib::VariantTy::UINT32));
    let weak = Rc::downgrade(terminal);
    workspace_select.connect_activate(move |_, parameter| {
        let Some(index) = parameter.and_then(|value| value.get::<u32>()) else {
            return;
        };
        if let Some(terminal) = weak.upgrade() {
            let effect = terminal
                .borrow_mut()
                .select_navigation_target(NavigationTarget::Workspace(index as usize));
            apply_navigation_effect(&terminal, effect);
        }
    });
    app.add_action(&workspace_select);

    let tab_select = gio::SimpleAction::new("select-tab", Some(glib::VariantTy::UINT32));
    let weak = Rc::downgrade(terminal);
    tab_select.connect_activate(move |_, parameter| {
        let Some(index) = parameter.and_then(|value| value.get::<u32>()) else {
            return;
        };
        if let Some(terminal) = weak.upgrade() {
            let effect = terminal
                .borrow_mut()
                .select_navigation_target(NavigationTarget::TerminalTab(index as usize));
            apply_navigation_effect(&terminal, effect);
        }
    });
    app.add_action(&tab_select);
}

pub(crate) fn build_menu_bar(
    app: &Application,
    terminal: &Rc<RefCell<Terminal>>,
    window: &ApplicationWindow,
    area: &GLArea,
) -> MenuState {
    register_menu_actions(app, terminal, window, area);

    let model = gio::Menu::new();
    let file = gio::Menu::new();
    append_command(&file, "New Terminal Tab", CommandId::TerminalNewTab);
    append_command(&file, "New Group", CommandId::GroupNew);
    append_command(&file, "New Workspace", CommandId::WorkspaceNew);
    file.append(Some("New Window"), Some("app.new-window"));
    append_command(&file, "Split Right", CommandId::PaneSplitRight);
    append_command(&file, "Split Down", CommandId::PaneSplitDown);
    file.append(Some("Connect over SSH…"), Some("app.ssh-connect"));
    append_command(&file, "Close Pane", CommandId::PaneClose);
    append_command(&file, "Close Tab", CommandId::TabClose);
    append_command(&file, "Close Group", CommandId::GroupClose);
    append_command(&file, "Close Workspace", CommandId::WorkspaceClose);
    append_command(&file, "Quit", CommandId::AppQuit);
    model.append_submenu(Some("File"), &file);

    let edit = gio::Menu::new();
    append_command(&edit, "Copy", CommandId::TerminalCopy);
    append_command(&edit, "Paste", CommandId::TerminalPaste);
    append_command(&edit, "Select All", CommandId::TerminalSelectAll);
    append_command(&edit, "Find…", CommandId::TerminalFind);
    append_command(&edit, "Find Next", CommandId::TerminalFindNext);
    append_command(&edit, "Find Previous", CommandId::TerminalFindPrevious);
    append_command(
        &edit,
        "Clear Scrollback",
        CommandId::TerminalClearScrollback,
    );
    append_command(
        &edit,
        "Set Resume Command…",
        CommandId::TerminalResumeCommand,
    );
    model.append_submenu(Some("Edit"), &edit);

    let view = gio::Menu::new();
    append_command(&view, "Toggle Sidebar", CommandId::AppToggleSidebar);
    append_command(&view, "Increase Font", CommandId::FontIncrease);
    append_command(&view, "Decrease Font", CommandId::FontDecrease);
    append_command(&view, "Reset Font", CommandId::FontReset);
    append_command(&view, "Next Tab", CommandId::TerminalNextTab);
    append_command(&view, "Previous Tab", CommandId::TerminalPreviousTab);
    append_command(&view, "Next Group", CommandId::GroupNext);
    append_command(&view, "Previous Group", CommandId::GroupPrevious);
    let focus = gio::Menu::new();
    append_command(&focus, "Left", CommandId::PaneFocusLeft);
    append_command(&focus, "Right", CommandId::PaneFocusRight);
    append_command(&focus, "Up", CommandId::PaneFocusUp);
    append_command(&focus, "Down", CommandId::PaneFocusDown);
    view.append_submenu(Some("Focus Pane"), &focus);
    let resize = gio::Menu::new();
    append_command(&resize, "Left", CommandId::PaneResizeLeft);
    append_command(&resize, "Right", CommandId::PaneResizeRight);
    append_command(&resize, "Up", CommandId::PaneResizeUp);
    append_command(&resize, "Down", CommandId::PaneResizeDown);
    view.append_submenu(Some("Resize Pane"), &resize);
    append_command(&view, "Zoom Pane", CommandId::PaneZoom);
    append_command(&view, "Full Screen", CommandId::AppFullScreen);
    model.append_submenu(Some("View"), &view);

    let workspace_menu = gio::Menu::new();
    let tab_menu = gio::Menu::new();
    let window_menu = gio::Menu::new();
    window_menu.append_submenu(Some("Workspaces"), &workspace_menu);
    window_menu.append_submenu(Some("Tabs"), &tab_menu);
    append_command(&window_menu, "Rename Workspace", CommandId::WorkspaceRename);
    append_command(&window_menu, "Rename Group", CommandId::GroupRename);
    append_command(&window_menu, "Rename Tab", CommandId::TerminalRenameTab);
    append_command(
        &window_menu,
        "Jump to Unread",
        CommandId::NotificationJumpUnread,
    );
    let move_focus = gio::Menu::new();
    append_command(&move_focus, "Left", CommandId::PaneFocusLeft);
    append_command(&move_focus, "Right", CommandId::PaneFocusRight);
    append_command(&move_focus, "Up", CommandId::PaneFocusUp);
    append_command(&move_focus, "Down", CommandId::PaneFocusDown);
    window_menu.append_submenu(Some("Move Pane Focus"), &move_focus);
    model.append_submenu(Some("Window"), &window_menu);

    let help = gio::Menu::new();
    help.append(Some("Kitmux Help"), Some("app.help-url"));
    help.append(Some("Keyboard Shortcuts"), Some("app.keyboard-shortcuts"));
    help.append(Some("Command Palette"), Some("app.command-palette"));
    append_command(
        &help,
        "Install Command Line Tool",
        CommandId::AppInstallCommandLineTool,
    );
    append_command(
        &help,
        "Reload Kitty Config",
        CommandId::AppReloadKittyConfig,
    );
    append_command(&help, "About Kitmux", CommandId::AppAbout);
    help.append(
        Some("Report an Issue"),
        Some(&command_action(CommandId::AppReportIssue)),
    );
    model.append_submenu(Some("Help"), &help);

    let help_url = gio::SimpleAction::new("help-url", None);
    help_url.connect_activate(|_, _| {
        open_url("https://digitalwestern.github.io/kitmux-website/".to_owned())
    });
    app.add_action(&help_url);

    set_menu_accelerators(app, &terminal.borrow().shortcuts);
    MenuState {
        model,
        workspace_menu,
        tab_menu,
    }
}

pub(crate) fn append_target(menu: &gio::Menu, label: &str, action: &str, index: usize) {
    let item = gio::MenuItem::new(Some(label), None);
    item.set_action_and_target_value(Some(action), Some(&(index as u32).to_variant()));
    menu.append_item(&item);
}

pub(crate) fn rebuild_window_menu(
    ui: &NavigationUi,
    workspaces: &[(WorkspaceId, String)],
    tabs: &[(TabId, String)],
) {
    ui.workspace_menu.remove_all();
    for (index, (_, name)) in workspaces.iter().take(9).enumerate() {
        append_target(
            &ui.workspace_menu,
            &format!("{}  {name}", index + 1),
            "app.select-workspace",
            index,
        );
    }
    if workspaces.len() > 9 {
        ui.workspace_menu.append(
            Some(&format!(
                "… {} more workspaces (use the sidebar)",
                workspaces.len() - 9
            )),
            None,
        );
    }

    ui.tab_menu.remove_all();
    for (index, (_, name)) in tabs.iter().take(9).enumerate() {
        append_target(
            &ui.tab_menu,
            &format!("{}  {name}", index + 1),
            "app.select-tab",
            index,
        );
    }
    if tabs.len() > 9 {
        ui.tab_menu.append(
            Some(&format!(
                "… {} more tabs (use the tab strip)",
                tabs.len() - 9
            )),
            None,
        );
    }
}

pub(crate) fn show_keyboard_shortcuts(
    window: &ApplicationWindow,
    terminal: &Rc<RefCell<Terminal>>,
) {
    let shortcuts = terminal.borrow().shortcuts.clone();
    let mut detail = CommandId::ALL
        .iter()
        .filter_map(|command| {
            Some(format!(
                "{}: {}",
                command.as_str(),
                shortcuts.accelerator_for_command(*command)?
            ))
        })
        .collect::<Vec<_>>();
    if let Some(accelerator) = shortcuts.accelerator_for_action(ShortcutAction::CommandPalette) {
        detail.push(format!("command-palette: {accelerator}"));
    }
    gtk::AlertDialog::builder()
        .message("Keyboard Shortcuts")
        .detail(detail.join("\n"))
        .buttons(["Close"])
        .build()
        .show(Some(window));
}

pub(crate) fn show_about(window: &ApplicationWindow) {
    let dialog = AboutDialog::new();
    dialog.set_transient_for(Some(window));
    dialog.set_modal(true);
    dialog.set_program_name(Some("Kitmux"));
    dialog.set_version(Some(env!("CARGO_PKG_VERSION")));
    dialog.set_license_type(License::Gpl30Only);
    dialog.set_comments(Some("Linux-first terminal multiplexer"));
    dialog.set_system_information(Some(
        "Bundled attribution notices: share/THIRD_PARTY.md and share/licenses/",
    ));
    dialog.set_website(Some("https://digitalwestern.github.io/kitmux-website/"));
    dialog.present();
}
