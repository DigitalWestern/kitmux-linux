#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
linux_root="$(cd "$script_dir/../.." && pwd)"
macos_repo="${KITMUX_MACOS_REPO:-$linux_root/../macos/kitmux}"
lock_file="$linux_root/source-lock.json"
destination="$linux_root/.source/reference"
expected_commit="e39381a0ed6c3d1667cb4dfa70e5bc48213b1bc4"
reference_tag="macos-linux-port-baseline-2026-07-23"

if [[ ! -d "$macos_repo/.git" ]]; then
  echo "macOS reference repository not found: $macos_repo" >&2
  exit 1
fi

actual_commit="$(git -C "$macos_repo" rev-list -n 1 "$reference_tag")"
if [[ "$actual_commit" != "$expected_commit" ]]; then
  echo "Reference tag mismatch: expected $expected_commit, got $actual_commit" >&2
  exit 1
fi

mkdir -p "$destination"
git -C "$macos_repo" archive "$reference_tag" libkitty patches \
  | tar -x -C "$destination"

python3 - "$destination" "$lock_file" <<'PY'
import hashlib
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
lock = json.loads(pathlib.Path(sys.argv[2]).read_text())
for relative, expected in lock["sha256"].items():
    path = root / relative
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    if actual != expected:
        raise SystemExit(f"hash mismatch for {relative}: {actual} != {expected}")
print(f"materialized and verified {len(lock['sha256'])} locked reference files")
PY

