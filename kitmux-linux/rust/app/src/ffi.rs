use std::ffi::{c_char, c_int, c_void};

pub enum KittyEngine {}
pub enum KittySession {}

#[repr(C)]
pub struct KittyEngineConfig {
    pub kitty_src_path: *const c_char,
    pub libkitty_py_path: *const c_char,
    pub python_home: *const c_char,
    pub config_path: *const c_char,
}

#[repr(C)]
pub struct KittySessionCallbacks {
    pub userdata: *mut c_void,
    pub on_damage: Option<unsafe extern "C" fn(*mut c_void)>,
    pub on_title: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    pub on_bell: Option<unsafe extern "C" fn(*mut c_void)>,
    pub on_child_exit: Option<unsafe extern "C" fn(*mut c_void, c_int)>,
    pub on_notification: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
    pub on_user_var: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
}

#[repr(C)]
pub struct KittyKeyEvent {
    pub key: u32,
    pub shifted_key: u32,
    pub alternate_key: u32,
    pub mods: u32,
    pub action: c_int,
    pub text: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct KitmuxGdkKeyInput {
    pub keyval: u32,
    pub unshifted_keyval: u32,
    pub state: u32,
    pub action: c_int,
}

#[repr(C)]
pub struct KitmuxKeyTranslation {
    pub event: KittyKeyEvent,
    pub text: [c_char; 32],
}

#[repr(C)]
#[derive(Default)]
pub struct KitmuxKeyTracker {
    pub codes: [u32; 32],
    pub count: usize,
}

unsafe extern "C" {
    pub fn kitty_engine_init(
        config: *const KittyEngineConfig,
        error: *mut c_char,
        error_len: usize,
    ) -> *mut KittyEngine;
    pub fn kitty_engine_shutdown(engine: *mut KittyEngine);
    pub fn kitty_render_init(
        engine: *mut KittyEngine,
        scale: f64,
        cell_width: *mut c_int,
        cell_height: *mut c_int,
        error: *mut c_char,
        error_len: usize,
    ) -> bool;
    pub fn kitty_session_create_with_options(
        engine: *mut KittyEngine,
        lines: c_int,
        columns: c_int,
        argv: *const *const c_char,
        cwd: *const c_char,
        env: *const *const c_char,
        callbacks: *const KittySessionCallbacks,
        error: *mut c_char,
        error_len: usize,
    ) -> *mut KittySession;
    pub fn kitty_session_close(session: *mut KittySession);
    pub fn kitty_session_child_alive(session: *mut KittySession) -> bool;
    pub fn kitty_session_child_pid(session: *mut KittySession) -> c_int;
    pub fn kitty_session_has_foreground_process(session: *mut KittySession) -> bool;
    pub fn kitty_session_fd(session: *mut KittySession) -> c_int;
    pub fn kitty_session_pump(session: *mut KittySession) -> bool;
    pub fn kitty_session_last_pump_bytes(session: *mut KittySession) -> usize;
    pub fn kitty_session_write(session: *mut KittySession, data: *const u8, length: usize);
    pub fn kitty_session_paste(session: *mut KittySession, data: *const u8, length: usize);
    pub fn kitty_session_scroll(session: *mut KittySession, lines: c_int);
    pub fn kitty_session_clear_scrollback(session: *mut KittySession);
    pub fn kitty_session_selection_start(
        session: *mut KittySession,
        column: u32,
        row: u32,
        in_left_half: bool,
        extend_mode: u32,
    );
    pub fn kitty_session_selection_update(
        session: *mut KittySession,
        column: u32,
        row: u32,
        in_left_half: bool,
        ended: bool,
    );
    pub fn kitty_session_selection_clear(session: *mut KittySession);
    pub fn kitty_session_selection_text(session: *mut KittySession) -> *mut c_char;
    pub fn kitty_session_search_set_options(
        session: *mut KittySession,
        query: *const c_char,
        query_len: usize,
        case_sensitive: bool,
        regex: bool,
        match_count: *mut usize,
        error: *mut c_char,
        error_len: usize,
    ) -> bool;
    pub fn kitty_session_search_next(session: *mut KittySession, backwards: bool) -> bool;
    pub fn kitty_session_search_clear(session: *mut KittySession);
    pub fn kitty_session_text(session: *mut KittySession) -> *mut c_char;
    pub fn kitty_session_line_wraps(
        session: *mut KittySession,
        output: *mut u8,
        capacity: usize,
    ) -> usize;
    pub fn kitty_session_mouse_event(
        session: *mut KittySession,
        cell_x: u32,
        cell_y: u32,
        button: c_int,
        action: c_int,
        mods: u32,
        pixel_x: c_int,
        pixel_y: c_int,
    ) -> c_int;
    pub fn kitty_session_encode_key(
        session: *mut KittySession,
        event: *const KittyKeyEvent,
        output: *mut c_char,
        output_len: usize,
    ) -> usize;
    pub fn kitty_session_reported_cwd(session: *mut KittySession) -> *mut c_char;
    pub fn kitty_render_font_size(engine: *mut KittyEngine) -> f64;
    pub fn kitty_render_set_font_size(
        engine: *mut KittyEngine,
        points: f64,
        cell_width: *mut c_int,
        cell_height: *mut c_int,
        error: *mut c_char,
        error_len: usize,
    ) -> bool;

    pub fn kitmux_translate_gdk_key(
        input: *const KitmuxGdkKeyInput,
        committed_text: *const c_char,
        output: *mut KitmuxKeyTranslation,
    ) -> bool;
    pub fn kitmux_key_tracker_press(tracker: *mut KitmuxKeyTracker, keycode: u32) -> c_int;
    pub fn kitmux_key_tracker_release(tracker: *mut KitmuxKeyTracker, keycode: u32) -> bool;
    pub fn kitmux_key_tracker_reset(tracker: *mut KitmuxKeyTracker);

    pub fn kitmux_product_terminal_area_new() -> *mut c_void;
    pub fn kitmux_gdk_base_layout_keyval(
        widget: *mut c_void,
        controller: *mut c_void,
        keycode: u32,
    ) -> u32;
}

pub const KEY_ACTION_RELEASE: c_int = 0;
pub const MOUSE_PRESS: c_int = 0;
pub const MOUSE_RELEASE: c_int = 1;
pub const MOUSE_DRAG: c_int = 2;
pub const MOUSE_MOVE: c_int = 3;
