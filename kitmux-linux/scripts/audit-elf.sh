#!/usr/bin/env bash
set -euo pipefail

library="${1:?usage: audit-elf.sh /path/to/libkitty.so}"

file "$library" | grep -q "ELF .* shared object"
readelf -d "$library" | grep -Fq 'Library runpath: [$ORIGIN]'

unexpected="$(
  nm -D --defined-only "$library" \
    | awk '{print $3}' \
    | grep -Ev '^(LIBKITTY_0|kitty_[A-Za-z0-9_]+@@LIBKITTY_0)$' \
    || true
)"
if [[ -n "$unexpected" ]]; then
  echo "Unexpected exported symbols:" >&2
  echo "$unexpected" >&2
  exit 1
fi

echo "ELF ABI audit: OK"
