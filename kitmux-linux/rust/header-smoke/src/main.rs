use std::ffi::{c_char, c_void};

#[repr(C)]
struct KittyEngineConfig {
    kitty_src_path: *const c_char,
    libkitty_py_path: *const c_char,
    python_home: *const c_char,
    config_path: *const c_char,
}

#[repr(C)]
struct KittySessionCallbacks {
    userdata: *mut c_void,
    on_damage: Option<unsafe extern "C" fn(*mut c_void)>,
    on_title: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    on_bell: Option<unsafe extern "C" fn(*mut c_void)>,
    on_child_exit: Option<unsafe extern "C" fn(*mut c_void, i32)>,
    on_notification:
        Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
    on_user_var:
        Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
}

#[repr(C)]
struct KittyKeyEvent {
    key: u32,
    shifted_key: u32,
    alternate_key: u32,
    mods: u32,
    action: i32,
    text: *const c_char,
}

#[repr(C)]
struct KitmuxGdkKeyInput {
    keyval: u32,
    unshifted_keyval: u32,
    state: u32,
    action: i32,
}

#[repr(C)]
struct KitmuxKeyTranslation {
    event: KittyKeyEvent,
    text: [c_char; 32],
}

#[repr(C)]
struct KitmuxKeyTracker {
    codes: [u32; 32],
    count: usize,
}

unsafe extern "C" {
    fn libkitty_engine_config_size() -> usize;
    fn libkitty_session_callbacks_size() -> usize;
    fn kitmux_gdk_key_input_size() -> usize;
    fn kitmux_key_translation_size() -> usize;
    fn kitmux_key_tracker_size() -> usize;
}

fn main() {
    unsafe {
        assert_eq!(
            size_of::<KittyEngineConfig>(),
            libkitty_engine_config_size()
        );
        assert_eq!(
            size_of::<KittySessionCallbacks>(),
            libkitty_session_callbacks_size()
        );
        assert_eq!(size_of::<KitmuxGdkKeyInput>(), kitmux_gdk_key_input_size());
        assert_eq!(
            size_of::<KitmuxKeyTranslation>(),
            kitmux_key_translation_size()
        );
        assert_eq!(size_of::<KitmuxKeyTracker>(), kitmux_key_tracker_size());
    }
}
