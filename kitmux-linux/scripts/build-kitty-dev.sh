#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
linux_root="$(cd "$script_dir/../.." && pwd)"
kitty="$linux_root/.source/kitty"

if [[ ! -d "$kitty/.git" ]]; then
  echo "Run scripts/fetch-kitty.sh first." >&2
  exit 1
fi

cd "$kitty"
./dev.sh build --skip-building-kitten
deps="$kitty/dependencies/linux-arm64"
env -u PYTHONHOME -u PYTHONPATH \
  LD_LIBRARY_PATH="$deps/lib" \
  "$deps/bin/python" -c \
  "import sys; sys.path.insert(0, '$kitty'); from kitty.fast_data_types import Screen; print('Linux fast_data_types: OK')"
