# ADR 0001: Rust is the initial Linux host language

Status: accepted for the engine and toolkit spikes

Rust is the default for the Linux model, lifecycle, and GTK host. It provides
clear ownership around long-lived sessions and a practical C FFI boundary to
libkitty.

This is not permission to rewrite libkitty or Kitty in Rust. The decision is
revisited only if the headless FFI or GTK spike reveals a concrete blocker.
