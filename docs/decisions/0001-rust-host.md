# ADR 0001: Rust is the initial Linux host language

Status: accepted for the future pure model and FFI harness; GUI host language
remains open

Rust is the default candidate for the Linux model and long-lived product
lifecycle. The Rust/C public layout check has passed and keeps the FFI boundary
practical.

The bounded GTK spike is currently a small C executable so it can test
libkitty and GTK directly without scaffolding a product architecture. That is
not a decision against Rust, and it is not permission to build the Rust model
before shared fixtures are authoritative.

This is not permission to rewrite libkitty or Kitty in Rust. Choose the final
GUI host language only after GTK or its fallback toolkit passes the Phase 2
decision gate.
