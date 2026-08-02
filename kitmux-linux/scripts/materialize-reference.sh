#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
linux_root="$(cd "$script_dir/../.." && pwd)"
macos_repo="${KITMUX_MACOS_REPO:-$linux_root/../macos/kitmux}"
lock_file="$linux_root/source-lock.json"
destination="$linux_root/.source/reference"
overlay_dir="$linux_root/kitmux-linux/patches/libkitty"
reference_lock="$(
  python3 - "$lock_file" <<'PY'
import json
import pathlib
import sys

reference = json.loads(pathlib.Path(sys.argv[1]).read_text())["macos_reference"]
print(reference["commit"])
print(reference["tag"])
PY
)"
expected_commit="${reference_lock%%$'\n'*}"
reference_tag="${reference_lock#*$'\n'}"

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

python3 - "$destination" "$lock_file" "$linux_root" <<'PY'
import hashlib
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
lock = json.loads(pathlib.Path(sys.argv[2]).read_text())
repo_root = pathlib.Path(sys.argv[3])
for relative, expected in lock["sha256"].items():
    path = root / relative
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    if actual != expected:
        raise SystemExit(f"hash mismatch for {relative}: {actual} != {expected}")
locked_overlays = lock.get("linux_overlays", {})
overlay_root = repo_root / "kitmux-linux" / "patches" / "libkitty"
actual_overlays = {
    path.relative_to(repo_root).as_posix() for path in overlay_root.glob("*.patch")
}
if actual_overlays != set(locked_overlays):
    missing = sorted(set(locked_overlays) - actual_overlays)
    extra = sorted(actual_overlays - set(locked_overlays))
    raise SystemExit(f"Linux overlay lock mismatch: missing={missing}, extra={extra}")
for relative, expected in locked_overlays.items():
    path = repo_root / relative
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    if actual != expected:
        raise SystemExit(
            f"hash mismatch for Linux overlay {relative}: {actual} != {expected}"
        )
print(
    f"materialized and verified {len(lock['sha256'])} locked reference files "
    f"and {len(locked_overlays)} Linux overlay"
)
PY

for overlay in "$overlay_dir"/*.patch; do
  [[ -f "$overlay" ]] || continue
  patch -d "$destination" -p1 --batch <"$overlay"
  echo "applied Linux overlay: $(basename "$overlay")"
done
