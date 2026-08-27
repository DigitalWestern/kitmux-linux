#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
linux_root="$(cd -- "${script_dir}/../.." && pwd)"
case "$(uname -m)" in
  aarch64|arm64) platform="linux-arm64" ;;
  x86_64|amd64) platform="linux-64" ;;
  *)
    echo "Unsupported Linux architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

if [[ -n "${KITMUX_DEPENDENCY_MIRROR:-}" ]]; then
  mirror="${KITMUX_DEPENDENCY_MIRROR}"
else
  durable_relative="$(python3 - "${linux_root}/source-lock.json" <<'PY'
import json
import pathlib
import sys

lock = json.loads(pathlib.Path(sys.argv[1]).read_text())
print(lock["durable_inputs"]["dependency_bundles"])
PY
)"
  mirror="${linux_root}/${durable_relative}"
fi
destination="${KITMUX_KITTY_DEPENDENCIES:-${linux_root}/.source/kitty/dependencies}"
mkdir -p "${destination}"

# Bundles are too large for git history; they are published as GitHub release
# assets and verified below against source-lock.json before use.
bundle_release_url="${KITMUX_BUNDLE_RELEASE_URL:-https://github.com/DigitalWestern/kitmux-linux/releases/download/dependency-bundles-v1}"
if [[ -d "${mirror}" || "${mirror}" == "${linux_root}/"* ]]; then
  mkdir -p "${mirror}"
  for name in "${platform}.tar.xz" "NerdFontsSymbolsOnly.tar.xz"; do
    if [[ ! -f "${mirror}/${name}" ]]; then
      echo "fetching ${name} from ${bundle_release_url}"
      if ! curl -fsSL --retry 3 -o "${mirror}/.${name}.download" "${bundle_release_url}/${name}"; then
        rm -f "${mirror}/.${name}.download"
        echo "could not download ${name}; continuing with local mirror contents" >&2
        continue
      fi
      mv "${mirror}/.${name}.download" "${mirror}/${name}"
    fi
  done
fi

python3 - "${linux_root}/source-lock.json" "${mirror}" "${destination}" \
  "${platform}" "${KITMUX_REQUIRE_DURABLE_INPUTS:-0}" <<'PY'
import hashlib
import json
import os
import pathlib
import shutil
import sys

lock_path, mirror_name, destination_name = map(pathlib.Path, sys.argv[1:4])
platform = sys.argv[4]
require_durable = sys.argv[5] == "1"
lock = json.loads(lock_path.read_text())
expected_bundles = lock.get("kitty_dependency_bundles", {})
durable_platforms = lock.get("durable_inputs", {}).get(
    "durable_dependency_platforms", list()
)
mirror = pathlib.Path(mirror_name)
destination = pathlib.Path(destination_name)

required = (f"{platform}.tar.xz", "NerdFontsSymbolsOnly.tar.xz")
require_durable = require_durable and platform in durable_platforms
for name in required:
    expected = expected_bundles.get(name)
    source = mirror / name
    if not expected:
        raise SystemExit(f"source-lock.json has no checksum for {name}")
    if source.is_file():
        digest = hashlib.sha256(source.read_bytes()).hexdigest()
        if digest != expected:
            raise SystemExit(
                f"dependency mirror hash mismatch for {name}: {digest} != {expected}"
            )

missing = [name for name in required if not (mirror / name).is_file()]
if missing:
    message = "durable dependency mirror is incomplete: " + ", ".join(missing)
    if require_durable:
        raise SystemExit(message)
    print(
        message
        + "; using the explicit unlocked dependency fallback for "
        + platform
    )
    raise SystemExit(0)

for name in required:
    expected = expected_bundles.get(name)
    source = mirror / name
    target = destination / name
    if target.is_file() and hashlib.sha256(target.read_bytes()).hexdigest() == expected:
        print(f"dependency already materialized: {name}")
        continue
    temporary = destination / f".{name}.tmp"
    shutil.copy2(source, temporary)
    os.replace(temporary, target)
    print(f"materialized locked dependency: {name}")
PY
