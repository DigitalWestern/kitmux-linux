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
deps_archive="$kitty/dependencies/linux-arm64.tar.xz"
fontconfig_library="$deps/lib/libfontconfig.so.1.16.1"

# A VirtioFS extraction on the macOS host once omitted this regular file while
# leaving its two symlinks behind. Repair it from Kitty's pinned dependency
# archive, then reject any other incomplete shared-library chain.
if [[ ! -f "$fontconfig_library" && -f "$deps_archive" ]]; then
  tar -xJf "$deps_archive" -C "$deps" lib/libfontconfig.so.1.16.1
fi
broken_links="$(find -L "$deps/lib" -maxdepth 1 -type l -print)"
if [[ -n "$broken_links" ]]; then
  echo "Broken Kitty dependency links:" >&2
  echo "$broken_links" >&2
  exit 1
fi

env -u PYTHONHOME -u PYTHONPATH \
  LD_LIBRARY_PATH="$deps/lib" \
  "$deps/bin/python" -c \
  "import sys; sys.path.insert(0, '$kitty'); from kitty.fast_data_types import Screen; print('Linux fast_data_types: OK')"
