mod control;
mod dialogs;
mod ffi;
mod menu;
mod navigation;
mod restore;
mod runtime;
mod ssh;
mod terminal;
mod window;

use gtk::Application;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use std::ffi::c_int;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) static TERMINATION_REQUESTED: AtomicBool = AtomicBool::new(false);

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

pub(crate) fn diagnostic(event: &str, fields: &[(&str, String)]) {
    eprint!("kitmux event={event}");
    for (key, value) in fields {
        eprint!(" {key}={value}");
    }
    eprintln!();
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
