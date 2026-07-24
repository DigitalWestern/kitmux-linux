#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
linux_root="$(cd "$script_dir/../.." && pwd)"
kitty="$linux_root/.source/kitty"

case "$(uname -m)" in
  aarch64|arm64) kitty_platform="linux-arm64" ;;
  x86_64|amd64) kitty_platform="linux-64" ;;
  *)
    echo "Unsupported Linux architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

if [[ ! -d "$kitty/.git" ]]; then
  echo "Run scripts/fetch-kitty.sh first." >&2
  exit 1
fi

cd "$kitty"
./dev.sh build --skip-building-kitten
deps="$kitty/dependencies/$kitty_platform"
deps_archive="$kitty/dependencies/$kitty_platform.tar.xz"
broken_links="$(find -L "$deps/lib" -maxdepth 1 -type l -print)"
if [[ -n "$broken_links" && -f "$deps_archive" ]]; then
  echo "Repairing an incomplete dependency tree from the locked archive."
  tar -xJf "$deps_archive" -C "$deps"
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
