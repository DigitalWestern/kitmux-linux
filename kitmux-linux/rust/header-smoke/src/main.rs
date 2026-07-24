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

unsafe extern "C" {
    fn libkitty_engine_config_size() -> usize;
    fn libkitty_session_callbacks_size() -> usize;
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
    }
}

