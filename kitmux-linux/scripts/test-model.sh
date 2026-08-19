#!/usr/bin/env bash
set -Eeuo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
source "$script_dir/gate-common.sh"
model_dir="$script_dir/../rust/model"

cd "$model_dir"
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets

if [ "$(uname -s)" = Linux ]; then
    cargo test --locked --test control_socket_tests
fi
