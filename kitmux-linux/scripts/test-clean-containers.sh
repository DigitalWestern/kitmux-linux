#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Run this script inside the Ubuntu headless VM." >&2
  exit 1
fi
if ! command -v podman >/dev/null 2>&1; then
  echo "Podman is required for clean Ubuntu and Fedora checks." >&2
  exit 1
fi

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
workspace="$(cd -- "${script_dir}/.." && pwd)"
linux_root="$(cd -- "${workspace}/.." && pwd)"
source_cache="${linux_root}/.source"

if [[ ! -f "${source_cache}/reference/LOCK.json" ]] \
    || [[ ! -f "${source_cache}/kitty/kitty/fast_data_types.so" ]]; then
  echo "Materialize the locked source and build Kitty before this gate." >&2
  exit 1
fi

podman build --pull=never \
  -t localhost/kitmux-clean-ubuntu:26.04 \
  -f "${workspace}/ci/Containerfile.ubuntu" "${workspace}/ci"
podman build --pull=never \
  -t localhost/kitmux-clean-fedora:44 \
  -f "${workspace}/ci/Containerfile.fedora" "${workspace}/ci"

base="$(mktemp -d /tmp/kitmux-clean-base.XXXXXX)"
cleanup() {
  rm -rf -- "${base}" "${base}".run-*
}
trap cleanup EXIT

git -C "${linux_root}" archive HEAD | tar -xf - -C "${base}"
tar -C "${linux_root}" \
  --exclude=.source/kitty/.git \
  -cf - .source \
  | tar -C "${base}" -xf -

run_clean_passes() {
  local label="$1"
  local image="$2"

  for pass in 1 2; do
    local run_root="${base}.run-${label}-${pass}"
    cp -al "${base}" "${run_root}"
    echo "Running ${label} clean release pass ${pass}/2"
    podman run --rm --user 0 \
      -v "${run_root}:/work" \
      -w /work \
      "${image}" \
      kitmux-linux/scripts/build-release-runtime.sh
    rm -rf -- "${run_root}"
  done
}

run_clean_passes ubuntu localhost/kitmux-clean-ubuntu:26.04
run_clean_passes fedora localhost/kitmux-clean-fedora:44

echo "Clean Ubuntu and Fedora release builds passed twice"
