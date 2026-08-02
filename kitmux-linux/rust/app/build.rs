use std::env;

fn main() {
    let native = env::var("KITMUX_NATIVE_LIB_DIR")
        .expect("KITMUX_NATIVE_LIB_DIR must name the CMake build directory");
    println!("cargo:rustc-link-search=native={native}");
    println!("cargo:rustc-link-lib=static=kitmux_terminal_bridge");
    println!("cargo:rustc-link-lib=static=kitmux_key_translation");
    println!("cargo:rustc-link-lib=dylib=kitty");
    println!("cargo:rustc-link-lib=dylib=epoxy");
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/app");
    println!("cargo:rerun-if-env-changed=KITMUX_NATIVE_LIB_DIR");
}
