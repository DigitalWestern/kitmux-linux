#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workspace="$(cd "$script_dir/.." && pwd)"
linux_root="$(cd "$workspace/.." && pwd)"

if [[ ! -f "$linux_root/.source/reference/libkitty/include/libkitty.h" ]]; then
  echo "Run scripts/materialize-reference.sh on the macOS host first." >&2
  exit 1
fi

"$script_dir/fetch-kitty.sh"
"$script_dir/build-kitty-dev.sh"

cmake -S "$workspace" -B "$workspace/build" -DCMAKE_BUILD_TYPE=RelWithDebInfo
cmake --build "$workspace/build" --parallel
ctest --test-dir "$workspace/build" --output-on-failure
"$script_dir/test-rust-header.sh"
python3 "$linux_root/contracts/validate-fixtures.py"
python3 "$linux_root/contracts/validate-inventory.py"
"$script_dir/test-model.sh"
