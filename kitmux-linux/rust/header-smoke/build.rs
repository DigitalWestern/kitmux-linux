use std::env;
use std::path::PathBuf;
use std::process::Command;

fn run(command: &mut Command, label: &str) {
    let status = command.status().expect(label);
    assert!(status.success(), "{label} failed with {status}");
}

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let include = manifest
        .join("../../../.source/reference/libkitty/include")
        .canonicalize()
        .expect("materialized libkitty include directory");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let object = output.join("header_probe.o");
    let archive = output.join("libheader_probe.a");
    let source = manifest.join("header_probe.c");

    run(
        Command::new("cc")
            .arg("-std=c17")
            .arg("-Wall")
            .arg("-Wextra")
            .arg("-Werror")
            .arg("-I")
            .arg(&include)
            .arg("-c")
            .arg(&source)
            .arg("-o")
            .arg(&object),
        "compile the public libkitty header for Rust",
    );
    run(
        Command::new("ar")
            .arg("crs")
            .arg(&archive)
            .arg(&object),
        "archive the Rust header probe",
    );

    println!("cargo:rustc-link-search=native={}", output.display());
    println!("cargo:rustc-link-lib=static=header_probe");
    println!("cargo:rerun-if-changed={}", source.display());
    println!(
        "cargo:rerun-if-changed={}",
        include.join("libkitty.h").display()
    );
}
