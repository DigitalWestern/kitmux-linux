use gtk::gdk;
use gtk::gio;
use gtk::glib::Propagation;
use gtk::prelude::*;
use gtk::{ApplicationWindow, Button, Entry, GLArea, Label};
use kitmux_model::{
    CommandId, PasteConfirmationReason, RestoreLayoutPolicy, ResumeCommandSelectionPolicy,
    command_palette_matches, paste_confirmation_reason, save_settings, valid_resume_command,
};
use std::cell::RefCell;
use std::env;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::diagnostic;
use crate::ffi;
use crate::menu::{palette_command_supported, show_about};
use crate::navigation::{
    NavigationEffect, RenameTarget, apply_navigation_effect, clear_box, foreground_scope,
    refresh_navigation,
};
use crate::terminal::Terminal;

pub(crate) static UNSAFE_PASTE_COUNT: AtomicUsize = AtomicUsize::new(0);
pub(crate) static FOREGROUND_CLOSE_COUNT: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn open_modal_dialog(terminal: &Rc<RefCell<Terminal>>) -> bool {
    let mut terminal = terminal.borrow_mut();
    if terminal.modal_dialog_open {
        return false;
    }
    terminal.modal_dialog_open = true;
    true
}

pub(crate) fn request_navigation_rename(
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

pub(crate) fn apply_navigation_command(
    terminal: &Rc<RefCell<Terminal>>,
    command: CommandId,
    reviewed: bool,
) {
    let effect = terminal.borrow_mut().navigation_action(command);
    if reviewed && matches!(effect, NavigationEffect::CloseWindow) {
        terminal.borrow_mut().close_confirmed = true;
    }
    apply_navigation_effect(terminal, effect);
}

pub(crate) fn request_navigation_command(
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
        CommandId::PaneClose
            | CommandId::TabClose
            | CommandId::GroupClose
            | CommandId::WorkspaceClose
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

pub(crate) fn request_resume_command(
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

pub(crate) fn present_resume_offers(
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

pub(crate) fn execute_palette_command(
    command: CommandId,
    terminal: &Rc<RefCell<Terminal>>,
    window: &ApplicationWindow,
    area: &GLArea,
) {
    match command {
        CommandId::AppQuit => window.close(),
        CommandId::AppHelp => {
            open_url("https://digitalwestern.github.io/kitmux-website/".to_owned())
        }
        CommandId::AppAbout => show_about(window),
        CommandId::AppReportIssue => {
            open_url("https://github.com/DigitalWestern/kitmux-website/issues".to_owned())
        }
        CommandId::TerminalSelectAll => terminal.borrow_mut().select_all(area),
        CommandId::TerminalFindNext | CommandId::TerminalFindPrevious => {
            let backwards = command == CommandId::TerminalFindPrevious;
            if !terminal.borrow_mut().navigate_search(backwards) {
                area.error_bell();
            }
        }
        CommandId::PaneZoom => terminal.borrow_mut().toggle_zoom(area),
        CommandId::AppFullScreen => terminal.borrow_mut().toggle_fullscreen(window),
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

pub(crate) fn populate_command_palette(
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

pub(crate) fn request_command_palette(
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

pub(crate) fn request_settings(
    window: &ApplicationWindow,
    area: &GLArea,
    terminal: &Rc<RefCell<Terminal>>,
) {
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
    let menu_bar = gtk::CheckButton::with_label("Show menu bar on launch");
    menu_bar.set_active(resolved.menu_bar_visible_on_launch);
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
        menu_bar.upcast_ref(),
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
        (menu_bar.upcast_ref(), "menu-bar"),
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
        resolved.menu_bar_visible_on_launch = menu_bar.is_active();
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

pub(crate) fn copy_selection(area: &GLArea, terminal: &Rc<RefCell<Terminal>>) {
    let Some(text) = terminal.borrow().selection_text() else {
        area.error_bell();
        return;
    };
    area.clipboard().set_text(&text);
    diagnostic("clipboard_copy", &[("bytes", text.len().to_string())]);
}

pub(crate) fn request_paste(
    window: &ApplicationWindow,
    area: &GLArea,
    terminal: &Rc<RefCell<Terminal>>,
) {
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

pub(crate) fn paste_reason(reason: PasteConfirmationReason) -> String {
    match reason {
        PasteConfirmationReason::Large { .. } => "large".to_owned(),
        PasteConfirmationReason::ControlCharacters => "controls".to_owned(),
    }
}

pub(crate) fn autoresume_decision() -> Option<&'static str> {
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

pub(crate) fn autopaste_decision() -> Option<bool> {
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

pub(crate) fn autoclose_decision() -> Option<bool> {
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

pub(crate) fn open_url(url: String) {
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
