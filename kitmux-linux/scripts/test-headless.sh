#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workspace="$(cd "$script_dir/.." && pwd)"
linux_root="$(cd "$workspace/.." && pwd)"
source "$script_dir/gate-common.sh"
build_dir="${KITMUX_HEADLESS_BUILD_DIR:-${KITMUX_BUILD_DIR:-${TMPDIR:-/tmp}/kitmux-linux-build-headless}}"

if [[ ! -f "$linux_root/.source/reference/libkitty/include/libkitty.h" ]]; then
  echo "Run scripts/materialize-reference.sh on the macOS host first." >&2
  exit 1
fi

"$script_dir/fetch-kitty.sh"
"$script_dir/build-kitty-dev.sh"

cmake -S "$workspace" -B "$build_dir" -DCMAKE_BUILD_TYPE=RelWithDebInfo
cmake --build "$build_dir" --parallel
ctest --test-dir "$build_dir" --output-on-failure
"$script_dir/test-rust-header.sh"
python3 "$linux_root/contracts/validate-fixtures.py"
"$script_dir/test-model.sh"
