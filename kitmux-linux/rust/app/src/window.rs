use super::{
    CommandId, ControlSocketError, KitmuxGdkKeyInput, NavigationEffect, NavigationUi,
    ShortcutAction, Terminal, apply_navigation_effect, attach_missing_pty_sources,
    attach_pty_source, attach_settings_source, attach_sigterm_source, autoclose_decision,
    build_menu_bar, changed, copy_selection, diagnostic, ffi, install_control_server, open_url,
    present_resume_offers, refresh_navigation, request_command_palette, request_navigation_command,
    request_paste, request_settings, run_accessibility_gate, run_navigation_gate_driver,
};
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::glib::Propagation;
use gtk::glib::translate::IntoGlib;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Button, Entry, GLArea, Label, SearchBar};
use kitmux_model::accumulate_scroll_lines;
use std::cell::{Cell, RefCell};
use std::env;
use std::ffi::c_int;
use std::rc::Rc;
use std::time::Duration;

fn attach_menu_key_diagnostics(
    controller: &gtk::EventControllerKey,
    menu_navigation: Rc<Cell<bool>>,
) {
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    controller.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::F10 {
            menu_navigation.set(true);
            diagnostic("menu_key", &[("key", "F10".to_owned())]);
        } else if menu_navigation.get()
            && matches!(
                key,
                gdk::Key::Left | gdk::Key::Right | gdk::Key::Up | gdk::Key::Down
            )
        {
            diagnostic("menu_key", &[("key", "arrow".to_owned())]);
        } else if menu_navigation.get() && key == gdk::Key::Escape {
            menu_navigation.set(false);
            diagnostic(
                "menu_keyboard_traversal",
                &[("roles", "true".to_owned()), ("focus", "true".to_owned())],
            );
        }
        Propagation::Proceed
    });
}

pub(super) fn build_window(app: &Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Kitmux")
        .default_width(900)
        .default_height(580)
        .build();
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let sidebar_shell = gtk::Box::new(gtk::Orientation::Vertical, 6);
    sidebar_shell.set_width_request(180);
    sidebar_shell.set_margin_start(8);
    sidebar_shell.set_margin_end(8);
    sidebar_shell.set_margin_top(8);
    sidebar_shell.set_margin_bottom(8);
    let workspace_header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let workspace_title = Label::new(Some("Workspaces"));
    workspace_title.set_xalign(0.0);
    workspace_title.set_hexpand(true);
    let workspace_new = Button::with_label("+");
    workspace_new.update_property(&[gtk::accessible::Property::Label("New workspace")]);
    workspace_header.append(&workspace_title);
    workspace_header.append(&workspace_new);
    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let workspace_controls = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let workspace_up = Button::with_label("↑");
    let workspace_down = Button::with_label("↓");
    let workspace_rename = Button::with_label("Rename");
    let workspace_close = Button::with_label("×");
    for control in [
        &workspace_up,
        &workspace_down,
        &workspace_rename,
        &workspace_close,
    ] {
        control.set_focus_on_click(false);
        workspace_controls.append(control);
    }
    sidebar_shell.append(&workspace_header);
    sidebar_shell.append(&sidebar);
    sidebar_shell.append(&workspace_controls);
    root.append(&sidebar_shell);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.set_hexpand(true);
    content.set_vexpand(true);
    let status = Label::new(Some("Initializing terminal…"));
    status.set_xalign(0.0);
    status.set_margin_start(12);
    status.set_margin_end(12);
    status.set_margin_top(8);
    status.set_margin_bottom(8);
    content.append(&status);

    let navigation_bar = gtk::Box::new(gtk::Orientation::Vertical, 4);
    navigation_bar.set_margin_start(8);
    navigation_bar.set_margin_end(8);
    navigation_bar.set_margin_bottom(6);
    let group_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let app_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    app_row.set_halign(gtk::Align::End);
    let tab_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let group_label = Label::new(Some("Group 1"));
    let group_previous = Button::with_label("‹");
    let group_next = Button::with_label("›");
    let group_new = Button::with_label("+ Group");
    let group_rename = Button::with_label("Rename");
    let group_close = Button::with_label("×");
    let command_palette = Button::with_label("Commands");
    command_palette.update_property(&[gtk::accessible::Property::Label("Open command palette")]);
    let settings = Button::with_label("Settings");
    settings.update_property(&[gtk::accessible::Property::Label("Open settings")]);
    let tab_strip = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    tab_strip.set_hexpand(true);
    let tab_previous = Button::with_label("←");
    let tab_next = Button::with_label("→");
    let tab_new = Button::with_label("+");
    let tab_rename = Button::with_label("Rename");
    let tab_close = Button::with_label("×");
    for control in [
        &group_previous,
        &group_next,
        &group_new,
        &group_rename,
        &group_close,
        &tab_previous,
        &tab_next,
        &tab_new,
        &tab_rename,
        &tab_close,
    ] {
        control.set_focus_on_click(false);
    }
    group_row.append(&group_label);
    group_row.append(&group_previous);
    group_row.append(&group_next);
    group_row.append(&group_new);
    group_row.append(&group_rename);
    group_row.append(&group_close);
    app_row.append(&command_palette);
    app_row.append(&settings);
    tab_row.append(&tab_strip);
    tab_row.append(&tab_previous);
    tab_row.append(&tab_next);
    tab_row.append(&tab_new);
    tab_row.append(&tab_rename);
    tab_row.append(&tab_close);
    navigation_bar.append(&group_row);
    navigation_bar.append(&app_row);
    navigation_bar.append(&tab_row);
    content.append(&navigation_bar);

    let search_bar = SearchBar::new();
    let search_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    search_row.set_margin_start(12);
    search_row.set_margin_end(12);
    search_row.set_margin_bottom(8);
    let search_entry = Entry::builder()
        .placeholder_text("Search terminal")
        .hexpand(true)
        .build();
    let search_count = Label::new(Some("0 matches"));
    let search_previous = Button::with_label("Previous");
    let search_next = Button::with_label("Next");
    let search_close = Button::with_label("Close");
    search_row.append(&search_entry);
    search_row.append(&search_count);
    search_row.append(&search_previous);
    search_row.append(&search_next);
    search_row.append(&search_close);
    search_bar.set_child(Some(&search_row));
    content.append(&search_bar);

    let area: GLArea = unsafe {
        glib::translate::from_glib_full(
            ffi::kitmux_product_terminal_area_new().cast::<gtk::ffi::GtkGLArea>(),
        )
    };
    area.set_allowed_apis(gdk::GLAPI::GL);
    area.set_required_version(3, 3);
    area.set_has_depth_buffer(false);
    area.set_has_stencil_buffer(false);
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_focusable(true);
    area.update_property(&[gtk::accessible::Property::Label("Terminal")]);
    content.append(&area);
    root.append(&content);
    let terminal = Rc::new(RefCell::new(Terminal::default()));
    let menus = build_menu_bar(app, &terminal, &window, &area);
    app.set_menubar(Some(&menus.model));
    window.set_show_menubar(true);
    let menu_bar = gtk::PopoverMenuBar::from_model(Some(&menus.model));
    menu_bar.set_focusable(true);
    menu_bar.update_property(&[gtk::accessible::Property::Label("Application menu")]);
    let chrome = gtk::Box::new(gtk::Orientation::Vertical, 0);
    chrome.append(&menu_bar);
    chrome.append(&root);
    window.set_child(Some(&chrome));
    terminal.borrow_mut().navigation_ui = Some(NavigationUi {
        app: app.downgrade(),
        sidebar_shell: sidebar_shell.downgrade(),
        sidebar: sidebar.downgrade(),
        tab_strip: tab_strip.downgrade(),
        group_label: group_label.downgrade(),
        status: status.downgrade(),
        window: window.downgrade(),
        area: area.downgrade(),
        search_bar: search_bar.downgrade(),
        search_entry: search_entry.downgrade(),
        menu_bar: menu_bar.downgrade(),
        workspace_menu: menus.workspace_menu,
        tab_menu: menus.tab_menu,
        command_palette: command_palette.downgrade(),
        settings: settings.downgrade(),
    });

    let menu_navigation = Rc::new(Cell::new(false));
    let menu_keys = gtk::EventControllerKey::new();
    attach_menu_key_diagnostics(&menu_keys, menu_navigation.clone());
    window.add_controller(menu_keys);
    let area_menu_keys = gtk::EventControllerKey::new();
    attach_menu_key_diagnostics(&area_menu_keys, menu_navigation.clone());
    area.add_controller(area_menu_keys);
    let menu_bar_keys = gtk::EventControllerKey::new();
    attach_menu_key_diagnostics(&menu_bar_keys, menu_navigation.clone());
    menu_bar.add_controller(menu_bar_keys);
    let menu_shortcuts = gtk::ShortcutController::new();
    menu_shortcuts.set_scope(gtk::ShortcutScope::Global);
    let menu_bar_f10 = menu_bar.clone();
    let menu_navigation_f10 = menu_navigation.clone();
    menu_shortcuts.add_shortcut(gtk::Shortcut::new(
        Some(gtk::KeyvalTrigger::new(
            gdk::Key::F10,
            gdk::ModifierType::NO_MODIFIER_MASK,
        )),
        Some(gtk::CallbackAction::new(move |_, _| {
            menu_navigation_f10.set(true);
            menu_bar_f10.grab_focus();
            let menu_navigation_popup = menu_navigation_f10.clone();
            glib::timeout_add_local_once(Duration::from_millis(50), move || {
                for toplevel in gtk::Window::list_toplevels() {
                    let popup_keys = gtk::EventControllerKey::new();
                    attach_menu_key_diagnostics(&popup_keys, menu_navigation_popup.clone());
                    toplevel.add_controller(popup_keys);
                }
            });
            diagnostic("menu_key", &[("key", "F10".to_owned())]);
            Propagation::Proceed
        })),
    ));
    for key in [
        gdk::Key::Left,
        gdk::Key::Right,
        gdk::Key::Up,
        gdk::Key::Down,
    ] {
        let menu_navigation = menu_navigation.clone();
        menu_shortcuts.add_shortcut(gtk::Shortcut::new(
            Some(gtk::KeyvalTrigger::new(
                key,
                gdk::ModifierType::NO_MODIFIER_MASK,
            )),
            Some(gtk::CallbackAction::new(move |_, _| {
                if menu_navigation.get() {
                    diagnostic("menu_key", &[("key", "arrow".to_owned())]);
                }
                Propagation::Proceed
            })),
        ));
    }
    let menu_navigation_escape = menu_navigation;
    menu_shortcuts.add_shortcut(gtk::Shortcut::new(
        Some(gtk::KeyvalTrigger::new(
            gdk::Key::Escape,
            gdk::ModifierType::NO_MODIFIER_MASK,
        )),
        Some(gtk::CallbackAction::new(move |_, _| {
            if menu_navigation_escape.get() {
                menu_navigation_escape.set(false);
                diagnostic(
                    "menu_keyboard_traversal",
                    &[("roles", "true".to_owned()), ("focus", "true".to_owned())],
                );
            }
            Propagation::Proceed
        })),
    ));
    window.add_controller(menu_shortcuts);
    if let Err(error) = install_control_server(&terminal) {
        match error {
            ControlSocketError::LiveServer => {
                diagnostic(
                    "control_server_declined",
                    &[("reason", "live_server".to_owned())],
                );
                terminal.borrow_mut().control_notice =
                    Some("local control handled by another instance".to_owned());
            }
            error => {
                let message = error.to_string();
                diagnostic("control_server_failed", &[("error", message.clone())]);
                terminal.borrow_mut().control_notice =
                    Some(format!("local control unavailable: {message}"));
            }
        }
    }

    let terminal_palette = terminal.clone();
    let window_palette = window.clone();
    let area_palette = area.clone();
    command_palette.connect_clicked(move |_| {
        request_command_palette(&window_palette, &area_palette, &terminal_palette);
    });
    let terminal_settings = terminal.clone();
    let window_settings = window.clone();
    let area_settings = area.clone();
    settings.connect_clicked(move |_| {
        request_settings(&window_settings, &area_settings, &terminal_settings);
    });

    let connect_action = |button: &Button, command: CommandId| {
        let weak = Rc::downgrade(&terminal);
        let window = window.clone();
        let area = area.clone();
        button.connect_clicked(move |_| {
            let Some(terminal) = weak.upgrade() else {
                return;
            };
            request_navigation_command(&terminal, command, &window, &area);
        });
    };
    connect_action(&workspace_new, CommandId::WorkspaceNew);
    connect_action(&workspace_rename, CommandId::WorkspaceRename);
    connect_action(&workspace_close, CommandId::WorkspaceClose);
    connect_action(&group_previous, CommandId::GroupPrevious);
    connect_action(&group_next, CommandId::GroupNext);
    connect_action(&group_new, CommandId::GroupNew);
    connect_action(&group_rename, CommandId::GroupRename);
    connect_action(&group_close, CommandId::GroupClose);
    connect_action(&tab_new, CommandId::TerminalNewTab);
    connect_action(&tab_rename, CommandId::TerminalRenameTab);
    connect_action(&tab_close, CommandId::PaneClose);

    let connect_move = |button: &Button, workspace: bool, direction: isize| {
        let weak = Rc::downgrade(&terminal);
        button.connect_clicked(move |_| {
            let Some(terminal) = weak.upgrade() else {
                return;
            };
            let moved = if workspace {
                terminal.borrow_mut().move_active_workspace(direction)
            } else {
                terminal.borrow_mut().move_active_tab(direction)
            };
            apply_navigation_effect(&terminal, changed(moved));
        });
    };
    connect_move(&workspace_up, true, -1);
    connect_move(&workspace_down, true, 1);
    connect_move(&tab_previous, false, -1);
    connect_move(&tab_next, false, 1);

    let terminal_search = terminal.clone();
    let search_count_changed = search_count.clone();
    search_entry.connect_changed(move |entry| {
        let result = terminal_search.borrow_mut().search(entry.text().as_str());
        match result {
            Ok(count) => search_count_changed.set_text(&format!("{count} matches")),
            Err(message) => search_count_changed.set_text(&message),
        }
    });
    let terminal_search_activate = terminal.clone();
    let area_search_activate = area.clone();
    search_entry.connect_activate(move |_| {
        if !terminal_search_activate.borrow_mut().navigate_search(false) {
            area_search_activate.error_bell();
        }
    });
    let terminal_search_next = terminal.clone();
    let area_search_next = area.clone();
    search_next.connect_clicked(move |_| {
        if !terminal_search_next.borrow_mut().navigate_search(false) {
            area_search_next.error_bell();
        }
    });
    let terminal_search_previous = terminal.clone();
    let area_search_previous = area.clone();
    search_previous.connect_clicked(move |_| {
        if !terminal_search_previous.borrow_mut().navigate_search(true) {
            area_search_previous.error_bell();
        }
    });
    let terminal_search_close = terminal.clone();
    let search_entry_close = search_entry.clone();
    let search_bar_close = search_bar.clone();
    let area_search_close = area.clone();
    search_close.connect_clicked(move |_| {
        search_entry_close.set_text("");
        terminal_search_close.borrow_mut().search("").ok();
        search_bar_close.set_search_mode(false);
        area_search_close.grab_focus();
    });
    let search_keys = gtk::EventControllerKey::new();
    let terminal_search_escape = terminal.clone();
    let search_entry_escape = search_entry.clone();
    let search_bar_escape = search_bar.clone();
    let area_search_escape = area.clone();
    search_keys.connect_key_pressed(move |_, key, _, _| {
        if key != gdk::Key::Escape {
            return Propagation::Proceed;
        }
        search_entry_escape.set_text("");
        terminal_search_escape.borrow_mut().search("").ok();
        search_bar_escape.set_search_mode(false);
        area_search_escape.grab_focus();
        Propagation::Stop
    });
    search_entry.add_controller(search_keys);
    let terminal_realize = terminal.clone();
    let window_realize = window.clone();
    let status_realize = status.clone();
    area.connect_realize(move |area| {
        let initialized = {
            terminal_realize
                .borrow_mut()
                .initialize(area, &window_realize, &status_realize)
        };
        match initialized {
            Ok(fd) => {
                let surface = terminal_realize.borrow().active_surface_id;
                if let Err(stage) = attach_pty_source(&terminal_realize, surface, fd) {
                    status_realize.set_text("Terminal event source failed");
                    diagnostic("terminal_init_failed", &[("stage", stage.to_owned())]);
                } else {
                    if let Err(stage) = attach_missing_pty_sources(&terminal_realize) {
                        status_realize.set_text("Restored terminal event source failed");
                        diagnostic("terminal_init_failed", &[("stage", stage.to_owned())]);
                        return;
                    }
                    attach_settings_source(&terminal_realize);
                    let weak = Rc::downgrade(&terminal_realize);
                    glib::idle_add_local_once(move || {
                        let Some(terminal) = weak.upgrade() else {
                            return;
                        };
                        refresh_navigation(&terminal);
                        diagnostic("navigation_ready", &[]);
                        let ui = terminal.borrow().navigation_ui.clone();
                        if let Some(ui) = ui
                            && let (Some(window), Some(area)) =
                                (ui.window.upgrade(), ui.area.upgrade())
                        {
                            present_resume_offers(&terminal, &window, &area);
                        }
                        if env::var_os("KITMUX_ACCESSIBILITY_GATE").is_some() {
                            run_accessibility_gate(&terminal);
                        }
                        if env::var_os("KITMUX_AUTONAVIGATION").is_some() {
                            let weak = Rc::downgrade(&terminal);
                            glib::timeout_add_local_once(Duration::from_millis(250), move || {
                                if let Some(terminal) = weak.upgrade() {
                                    run_navigation_gate_driver(&terminal);
                                }
                            });
                        }
                    });
                }
            }
            Err(stage) => {
                status_realize.set_text(&format!("Terminal unavailable: {stage}"));
                diagnostic("terminal_init_failed", &[("stage", stage)]);
            }
        }
    });

    let terminal_render = terminal.clone();
    let status_render = status.clone();
    area.connect_render(move |area, _context| {
        if let Ok(mut terminal) = terminal_render.try_borrow_mut() {
            terminal.render(area, &status_render);
        }
        Propagation::Stop
    });

    let im_context = gtk::IMMulticontext::new();
    im_context.set_client_widget(Some(&area));
    im_context.set_use_preedit(true);
    let terminal_commit = terminal.clone();
    im_context.connect_commit(move |_, text| {
        if let Ok(mut terminal) = terminal_commit.try_borrow_mut() {
            terminal.im_commit(text);
        }
    });
    let terminal_preedit_start = terminal.clone();
    im_context.connect_preedit_start(move |_| {
        if let Ok(mut terminal) = terminal_preedit_start.try_borrow_mut() {
            terminal.preedit_active = true;
        }
    });
    let terminal_preedit_end = terminal.clone();
    im_context.connect_preedit_end(move |_| {
        if let Ok(mut terminal) = terminal_preedit_end.try_borrow_mut() {
            terminal.preedit_active = false;
        }
    });

    let terminal_press = terminal.clone();
    let area_press = area.clone();
    let window_press = window.clone();
    let search_bar_press = search_bar.clone();
    let search_entry_press = search_entry.clone();
    let im_press = im_context.clone();
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(move |controller, keyval, keycode, state| {
        if keyval == gdk::Key::F4 && state.contains(gdk::ModifierType::ALT_MASK) {
            window_press.close();
            return Propagation::Stop;
        }
        let shortcut = { terminal_press.borrow().shortcut(keyval, state) };
        if let Some(shortcut) = shortcut {
            let first_press = unsafe {
                ffi::kitmux_key_tracker_press(
                    &mut terminal_press.borrow_mut().shortcut_consumed,
                    keycode,
                ) == 1
            };
            if first_press {
                match shortcut {
                    ShortcutAction::Copy => copy_selection(&area_press, &terminal_press),
                    ShortcutAction::Paste => {
                        request_paste(&window_press, &area_press, &terminal_press)
                    }
                    ShortcutAction::Search => {
                        search_bar_press.set_search_mode(true);
                        search_entry_press.grab_focus();
                    }
                    ShortcutAction::CommandPalette => {
                        request_command_palette(&window_press, &area_press, &terminal_press)
                    }
                    ShortcutAction::FontLarger => terminal_press
                        .borrow_mut()
                        .change_font_size(&area_press, 2.0),
                    ShortcutAction::FontSmaller => terminal_press
                        .borrow_mut()
                        .change_font_size(&area_press, -2.0),
                    ShortcutAction::FontReset => {
                        let size = terminal_press.borrow().default_font_size;
                        terminal_press.borrow_mut().set_font_size(&area_press, size);
                    }
                    ShortcutAction::ClearScrollback => {
                        let mut terminal = terminal_press.borrow_mut();
                        terminal.clear_selection();
                        if !terminal.session.is_null() {
                            unsafe { ffi::kitty_session_clear_scrollback(terminal.session) };
                            area_press.queue_render();
                        }
                    }
                    ShortcutAction::Navigation(command) => {
                        request_navigation_command(
                            &terminal_press,
                            command,
                            &window_press,
                            &area_press,
                        );
                    }
                    ShortcutAction::Select(target) => {
                        let effect = terminal_press.borrow_mut().select_navigation_target(target);
                        apply_navigation_effect(&terminal_press, effect);
                    }
                }
            }
            return Propagation::Stop;
        }
        let input = {
            let mut terminal = terminal_press.borrow_mut();
            let action = unsafe { ffi::kitmux_key_tracker_press(&mut terminal.keys, keycode) };
            let input = KitmuxGdkKeyInput {
                keyval: keyval.into_glib(),
                unshifted_keyval: unsafe {
                    ffi::kitmux_gdk_base_layout_keyval(
                        area_press.as_ptr().cast(),
                        controller.as_ptr().cast(),
                        keycode,
                    )
                },
                state: state.bits(),
                action,
            };
            terminal.filtering = true;
            terminal.filtering_input = input;
            terminal.filtering_had_preedit = terminal.preedit_active;
            terminal.filtering_committed = false;
            terminal.filtering_encoded = false;
            input
        };
        let consumed = controller
            .current_event()
            .is_some_and(|event| im_press.filter_keypress(event));
        let mut terminal = terminal_press.borrow_mut();
        let encoded = terminal.filtering_encoded;
        terminal.filtering = false;
        if consumed {
            if !encoded {
                unsafe { ffi::kitmux_key_tracker_press(&mut terminal.im_consumed, keycode) };
            }
            return Propagation::Stop;
        }
        terminal.route_key(&input, None);
        Propagation::Stop
    });
    let terminal_release = terminal.clone();
    let area_release = area.clone();
    keys.connect_key_released(move |controller, keyval, keycode, state| {
        let mut terminal = terminal_release.borrow_mut();
        if unsafe { ffi::kitmux_key_tracker_release(&mut terminal.shortcut_consumed, keycode) } {
            return;
        }
        unsafe { ffi::kitmux_key_tracker_release(&mut terminal.keys, keycode) };
        if unsafe { ffi::kitmux_key_tracker_release(&mut terminal.im_consumed, keycode) } {
            return;
        }
        let input = KitmuxGdkKeyInput {
            keyval: keyval.into_glib(),
            unshifted_keyval: unsafe {
                ffi::kitmux_gdk_base_layout_keyval(
                    area_release.as_ptr().cast(),
                    controller.as_ptr().cast(),
                    keycode,
                )
            },
            state: state.bits(),
            action: ffi::KEY_ACTION_RELEASE,
        };
        terminal.route_key(&input, None);
    });
    area.add_controller(keys);

    let focus = gtk::EventControllerFocus::new();
    let im_focus_in = im_context.clone();
    focus.connect_enter(move |_| im_focus_in.focus_in());
    let im_focus_out = im_context;
    let terminal_focus_out = terminal.clone();
    focus.connect_leave(move |_| {
        im_focus_out.focus_out();
        if let Ok(mut terminal) = terminal_focus_out.try_borrow_mut() {
            unsafe {
                ffi::kitmux_key_tracker_reset(&mut terminal.keys);
                ffi::kitmux_key_tracker_reset(&mut terminal.im_consumed);
                ffi::kitmux_key_tracker_reset(&mut terminal.shortcut_consumed);
            }
        }
    });
    area.add_controller(focus);

    let divider_drag = gtk::GestureDrag::new();
    divider_drag.set_button(1);
    let terminal_drag_begin = terminal.clone();
    let area_drag_begin = area.clone();
    divider_drag.connect_drag_begin(move |gesture, x, y| {
        let mut terminal = terminal_drag_begin.borrow_mut();
        if let Some(split) = terminal.divider_at(&area_drag_begin, x, y) {
            terminal.divider_drag = Some((split, x, y));
            gesture.set_state(gtk::EventSequenceState::Claimed);
        }
    });
    let terminal_drag_update = terminal.clone();
    let area_drag_update = area.clone();
    divider_drag.connect_drag_update(move |_, offset_x, offset_y| {
        let mut terminal = terminal_drag_update.borrow_mut();
        let Some((split, start_x, start_y)) = terminal.divider_drag else {
            return;
        };
        if terminal.resize_divider(
            &area_drag_update,
            split,
            start_x + offset_x,
            start_y + offset_y,
        ) {
            area_drag_update.queue_render();
        }
    });
    let terminal_drag_end = terminal.clone();
    divider_drag.connect_drag_end(move |_, _, _| {
        if let Some((split, _, _)) = terminal_drag_end.borrow_mut().divider_drag.take() {
            diagnostic("divider_resized", &[("split", split.to_string())]);
        }
    });
    area.add_controller(divider_drag);

    let click = gtk::GestureClick::new();
    click.set_button(0);
    let terminal_click = terminal.clone();
    let area_click = area.clone();
    click.connect_pressed(move |gesture, count, x, y| {
        area_click.grab_focus();
        let button = gesture.current_button() as c_int;
        let state = gesture.current_event_state();
        let divider = (button == 1)
            .then(|| terminal_click.borrow().divider_at(&area_click, x, y))
            .flatten();
        if env::var_os("KITMUX_INTERACTION_DIAGNOSTICS").is_some() {
            diagnostic(
                "pointer_press",
                &[
                    ("button", button.to_string()),
                    ("x", format!("{x:.1}")),
                    ("y", format!("{y:.1}")),
                    ("divider", divider.is_some().to_string()),
                ],
            );
        }
        if divider.is_some() {
            return;
        }
        let focused = terminal_click.borrow_mut().focus_pane_at(&area_click, x, y);
        if focused {
            diagnostic("pane_focused", &[("source", "pointer".to_owned())]);
            apply_navigation_effect(&terminal_click, NavigationEffect::Changed);
        }
        if button == 1
            && state.contains(gdk::ModifierType::CONTROL_MASK)
            && let Some(url) = terminal_click.borrow().url_at(&area_click, x, y)
        {
            open_url(url);
            gesture.set_state(gtk::EventSequenceState::Claimed);
            return;
        }
        let mut terminal = terminal_click.borrow_mut();
        terminal.mouse_reporting_button = None;
        terminal.selection_active = false;
        if !state.contains(gdk::ModifierType::SHIFT_MASK)
            && terminal.send_mouse(&area_click, x, y, button, ffi::MOUSE_PRESS, state)
        {
            terminal.mouse_reporting_button = Some(button);
        } else if button == 1 {
            terminal.start_selection(&area_click, x, y, count);
        }
    });
    let terminal_release_pointer = terminal.clone();
    let area_release_pointer = area.clone();
    click.connect_released(move |gesture, _, x, y| {
        let state = gesture.current_event_state();
        let mut terminal = terminal_release_pointer.borrow_mut();
        if let Some(button) = terminal.mouse_reporting_button.take() {
            terminal.send_mouse(
                &area_release_pointer,
                x,
                y,
                button,
                ffi::MOUSE_RELEASE,
                state,
            );
        } else {
            terminal.update_selection(&area_release_pointer, x, y, true);
        }
    });
    area.add_controller(click);

    let motion = gtk::EventControllerMotion::new();
    let terminal_motion = terminal.clone();
    let area_motion = area.clone();
    motion.connect_motion(move |controller, x, y| {
        let state = controller.current_event_state();
        let mut terminal = terminal_motion.borrow_mut();
        if let Some(button) = terminal.mouse_reporting_button {
            terminal.send_mouse(&area_motion, x, y, button, ffi::MOUSE_DRAG, state);
        } else if terminal.selection_active {
            terminal.update_selection(&area_motion, x, y, false);
        } else if !state.contains(gdk::ModifierType::SHIFT_MASK) {
            terminal.send_mouse(&area_motion, x, y, -1, ffi::MOUSE_MOVE, state);
        }
    });
    area.add_controller(motion);

    let scroll = gtk::EventControllerScroll::new(
        gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::KINETIC,
    );
    scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
    let terminal_scroll = terminal.clone();
    let area_scroll = area.clone();
    scroll.connect_scroll(move |controller, _, dy| {
        if env::var_os("KITMUX_INTERACTION_DIAGNOSTICS").is_some() {
            diagnostic("scroll_raw", &[("dy", format!("{dy:.3}"))]);
        }
        let Some(event) = controller.current_event() else {
            return Propagation::Proceed;
        };
        let Some((x, y)) = event.position() else {
            return Propagation::Proceed;
        };
        let state = controller.current_event_state();
        let mut terminal = terminal_scroll.borrow_mut();
        let scale = f64::from(area_scroll.scale_factor()).max(1.0);
        let cell_points = f64::from(terminal.cell_height.max(1)) / scale;
        let direction = event
            .downcast_ref::<gdk::ScrollEvent>()
            .map(gdk::ScrollEvent::direction);
        let mouse_wheel = event
            .device()
            .is_some_and(|device| device.source() == gdk::InputSource::Mouse);
        let delta_points = match (direction, mouse_wheel) {
            (Some(gdk::ScrollDirection::Up | gdk::ScrollDirection::Down), _) | (_, true) => {
                -dy * cell_points * terminal.wheel_scroll_lines as f64
            }
            _ => -dy,
        };
        let lines =
            accumulate_scroll_lines(delta_points, cell_points, &mut terminal.scroll_residue);
        if lines == 0 || terminal.session.is_null() {
            return Propagation::Stop;
        }
        if env::var_os("KITMUX_INTERACTION_DIAGNOSTICS").is_some() {
            diagnostic("scroll", &[("lines", lines.to_string())]);
        }
        let button = if lines > 0 { 4 } else { 5 };
        if !state.contains(gdk::ModifierType::SHIFT_MASK)
            && terminal.send_mouse(&area_scroll, x, y, button, ffi::MOUSE_PRESS, state)
        {
            for _ in 1..lines.unsigned_abs() {
                terminal.send_mouse(&area_scroll, x, y, button, ffi::MOUSE_PRESS, state);
            }
        } else {
            unsafe { ffi::kitty_session_scroll(terminal.session, lines) };
            area_scroll.queue_render();
        }
        Propagation::Stop
    });
    area.add_controller(scroll);

    let terminal_close = terminal.clone();
    let area_close = area.clone();
    window.connect_close_request(move |window| {
        let Ok(mut current) = terminal_close.try_borrow_mut() else {
            let retry_window = window.clone();
            glib::idle_add_local_once(move || retry_window.close());
            return Propagation::Stop;
        };
        let foreground = current.foreground_surfaces(None);
        if current.close_confirmed
            || !current.confirm_close_with_running_process
            || foreground.is_empty()
        {
            current.shutdown(&area_close);
            return Propagation::Proceed;
        }
        if current.modal_dialog_open {
            return Propagation::Stop;
        }
        if let Some(confirm) = autoclose_decision() {
            if confirm {
                current.close_confirmed = true;
                diagnostic(
                    "close_confirmed",
                    &[
                        ("foreground_rechecked", "true".to_owned()),
                        ("sessions", foreground.len().to_string()),
                    ],
                );
                current.shutdown(&area_close);
                return Propagation::Proceed;
            }
            diagnostic("close_cancelled", &[]);
            return Propagation::Stop;
        }
        current.modal_dialog_open = true;
        drop(current);
        let dialog = gtk::AlertDialog::builder()
            .modal(true)
            .message("Close a terminal with a running process?")
            .detail("Closing will terminate the foreground process and its shell.")
            .buttons(["Cancel", "Close"])
            .cancel_button(0)
            .default_button(0)
            .build();
        let terminal_confirm = terminal_close.clone();
        let window_confirm = window.clone();
        let area_confirm = area_close.clone();
        dialog.choose(Some(window), None::<&gio::Cancellable>, move |choice| {
            let mut terminal = terminal_confirm.borrow_mut();
            terminal.modal_dialog_open = false;
            if matches!(choice, Ok(1)) {
                terminal.close_confirmed = true;
                let foreground = terminal.foreground_surfaces(None).len();
                diagnostic(
                    "close_confirmed",
                    &[
                        ("foreground_rechecked", (foreground > 0).to_string()),
                        ("sessions", foreground.to_string()),
                    ],
                );
                drop(terminal);
                window_confirm.close();
            } else {
                drop(terminal);
                area_confirm.grab_focus();
                diagnostic("close_cancelled", &[]);
            }
        });
        Propagation::Stop
    });

    let terminal_unrealize = terminal.clone();
    area.connect_unrealize(move |area| {
        if let Ok(mut terminal) = terminal_unrealize.try_borrow_mut() {
            terminal.shutdown(area);
        }
    });

    attach_sigterm_source(&terminal, &window, &area);

    window.present();
}
