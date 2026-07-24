#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workspace="$(cd "$script_dir/.." && pwd)"

cargo run --quiet --manifest-path "$workspace/rust/header-smoke/Cargo.toml"
echo "Rust/C libkitty header layout: OK"
