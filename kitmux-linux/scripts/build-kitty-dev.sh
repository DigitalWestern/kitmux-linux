#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
linux_root="$(cd "$script_dir/../.." && pwd)"
kitty="$linux_root/.source/kitty"

case "$(uname -m)" in
  aarch64|arm64)
    kitty_platform="linux-arm64"
    kitty_develop_platform="linux-arm64"
    ;;
  x86_64|amd64)
    kitty_platform="linux-64"
    # Kitty's devenv names the amd64 extraction root linux-amd64, while the
    # locked bundle and our CMake contract use linux-64.
    kitty_develop_platform="linux-amd64"
    ;;
  *)
    echo "Unsupported Linux architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

if [[ ! -d "$kitty/.git" ]]; then
  echo "Run scripts/fetch-kitty.sh first." >&2
  exit 1
fi

"$script_dir/materialize-dependencies.sh"

deps="$kitty/dependencies/$kitty_platform"
develop_deps="$kitty/dependencies/$kitty_develop_platform"
deps_archive="$kitty/dependencies/$kitty_platform.tar.xz"
locked_archive=0

if [[ -f "$deps_archive" ]]; then
  expected_archive="$(python3 - "$linux_root/source-lock.json" "$kitty_platform" <<'PY'
import json
import pathlib
import sys

lock = json.loads(pathlib.Path(sys.argv[1]).read_text())
print(lock["kitty_dependency_bundles"][f"{sys.argv[2]}.tar.xz"])
PY
)"
  if [[ "$(sha256sum "$deps_archive" | awk '{print $1}')" == "$expected_archive" ]]; then
    locked_archive=1
  fi
fi

# Kitty's `dev.sh deps` rewrites the bundle's /sw/sw build prefix in every
# pkg-config and _sysconfigdata file and drops the bundled libfontconfig so
# the system one is used. The locked-archive path skips `dev.sh deps`, so a
# clean checkout must repeat both after extraction or the build compiles
# against nonexistent /sw/sw include paths.
relocate_dependency_tree() {
  local root="$1"
  find "$root" -type f \( -name '*.pc' -o -name '_sysconfigdata_*.py' \) \
    -exec sed -i "s|/sw/sw|$root|g" {} +
  find "$root/lib" -maxdepth 1 -name 'libfontconfig.so*' -delete 2>/dev/null || true
}

if [[ ! -x "$develop_deps/bin/python" ]]; then
  if [[ -f "$deps_archive" ]]; then
    mkdir -p "$develop_deps"
    tar -xJf "$deps_archive" -C "$develop_deps"
    relocate_dependency_tree "$develop_deps"
  elif [[ "${KITMUX_ALLOW_SOURCE_DEPENDENCY_BUILD:-0}" == "1" ]]; then
    (cd "$kitty" && ./dev.sh deps)
  else
    echo "Kitty development dependencies are missing: $develop_deps" >&2
    exit 1
  fi
fi

if [[ "$kitty_platform" == "linux-64" && ! -e "$deps" ]]; then
  ln -s "$kitty_develop_platform" "$deps"
fi

# Kitty's `dev.sh deps` normally extracts the builtin symbols font; the locked
# path skips it, so a clean checkout needs the same extraction or setup.py
# falls back to fc-list and fails on hosts without the font installed.
fonts_archive="$kitty/dependencies/NerdFontsSymbolsOnly.tar.xz"
if [[ ! -f "$kitty/fonts/SymbolsNerdFontMono-Regular.ttf" && -f "$fonts_archive" ]]; then
  mkdir -p "$kitty/fonts"
  tar -xf "$fonts_archive" -C "$kitty/fonts" SymbolsNerdFontMono-Regular.ttf
fi

cd "$kitty"
./dev.sh build --skip-building-kitten
# Kitty deliberately uses the system fontconfig on Linux.  Its dependency
# fallback removes the targets but leaves these two symlinks behind.
find -L "$deps/lib" -maxdepth 1 -type l -name 'libfontconfig.so*' -delete
broken_links="$(find -L "$deps/lib" -maxdepth 1 -type l -print)"
if [[ -n "$broken_links" && "$locked_archive" == 1 ]]; then
  echo "Repairing an incomplete dependency tree from the locked archive."
  tar -xJf "$deps_archive" -C "$deps"
  relocate_dependency_tree "$deps"
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
