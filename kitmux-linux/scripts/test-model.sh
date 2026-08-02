#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
model_dir="$script_dir/../rust/model"

cd "$model_dir"
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets

if [ "$(uname -s)" = Linux ]; then
    cargo test --locked --test control_socket_tests
fi
