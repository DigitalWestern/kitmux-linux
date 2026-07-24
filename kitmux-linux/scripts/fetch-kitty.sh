#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
linux_root="$(cd "$script_dir/../.." && pwd)"
reference="$linux_root/.source/reference"
kitty="$linux_root/.source/kitty"
expected_commit="c1d507dbe8cd12830d8b97b0d350d9dc2e4d383f"

if [[ ! -d "$reference/patches" ]]; then
  echo "Materialize the locked reference first." >&2
  exit 1
fi

if [[ ! -d "$kitty/.git" ]]; then
  git init "$kitty"
  git -C "$kitty" remote add origin https://github.com/kovidgoyal/kitty.git
  git -C "$kitty" fetch --depth 1 origin "$expected_commit"
  git -C "$kitty" checkout --detach FETCH_HEAD
fi

actual_commit="$(git -C "$kitty" rev-parse HEAD)"
if [[ "$actual_commit" != "$expected_commit" ]]; then
  echo "Kitty source mismatch: expected $expected_commit, got $actual_commit" >&2
  exit 1
fi

for patch in "$reference"/patches/*.patch; do
  patch_name="$(basename "$patch")"
  if [[ "$patch_name" == "0001-libkitty-render-exports.patch" ]] \
      && grep -q "init_libkitty_render" "$kitty/kitty/data-types.c"; then
    echo "already applied: $patch_name"
    continue
  fi
  if [[ "$patch_name" == "0002-kitmux-default-zsh-prompt.patch" ]] \
      && grep -q "KITMUX_DEFAULT_PROMPT" "$kitty/shell-integration/zsh/kitty-integration"; then
    echo "already applied: $patch_name"
    continue
  fi
  if git -C "$kitty" apply --check "$patch" 2>/dev/null; then
    git -C "$kitty" apply "$patch"
    echo "applied $patch_name"
  elif git -C "$kitty" apply --reverse --check "$patch" 2>/dev/null; then
    echo "already applied: $patch_name"
  else
    echo "Patch conflict: $patch" >&2
    exit 1
  fi
done

python3 "$script_dir/port-render-loader.py" "$kitty/kitty/libkitty_render.c"

echo "Kitty source ready at $actual_commit"
