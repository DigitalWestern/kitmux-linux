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
source "${script_dir}/gate-common.sh"
source_cache="${linux_root}/.source"

if [[ ! -f "${linux_root}/source-lock.json" ]] \
    || [[ ! -f "${source_cache}/reference/libkitty/include/libkitty.h" ]] \
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

# Copy the candidate worktree, including intentional uncommitted files, into an
# isolated tree. Using `git archive HEAD` here previously tested the last
# commit instead of the changes about to be committed.
git -C "${linux_root}" ls-files --cached --others --exclude-standard -z \
  | tar -C "${linux_root}" --null -T - -cf - \
  | tar -xf - -C "${base}"
tar -C "${linux_root}" \
  --exclude=.source/kitty/.git \
  -cf - .source \
  | tar -C "${base}" -xf -

run_clean_passes() {
  local label="$1"
  local image="$2"
  local first_inventory_hash=""

  for pass in 1 2; do
    local run_root="${base}.run-${label}-${pass}"
    cp -al "${base}" "${run_root}"
    echo "Running ${label} clean release pass ${pass}/2"
    podman run --rm --user 0 \
      -v "${run_root}:/work" \
      -w /work \
      "${image}" \
      env KITMUX_REQUIRE_DURABLE_INPUTS=1 \
      KITMUX_ALLOW_SOURCE_DEPENDENCY_BUILD=1 \
      KITMUX_INVENTORY_VALIDATED=1 \
      kitmux-linux/scripts/build-release-runtime.sh \
      kitmux-linux/build/kitmux-engine-runtime
    local inventory_hash
    inventory_hash="$(
      sha256sum "${run_root}/kitmux-linux/build/kitmux-engine-runtime/share/SHA256SUMS" \
        | awk '{print $1}'
    )"
    if [[ "${pass}" -eq 1 ]]; then
      first_inventory_hash="${inventory_hash}"
    elif [[ "${inventory_hash}" != "${first_inventory_hash}" ]]; then
      echo "${label} release contents changed between identical clean passes." >&2
      exit 1
    fi
    rm -rf -- "${run_root}"
  done
  echo "${label} reproducible release inventory: ${first_inventory_hash}"
}

run_clean_passes ubuntu localhost/kitmux-clean-ubuntu:26.04
run_clean_passes fedora localhost/kitmux-clean-fedora:44

echo "Clean Ubuntu and Fedora release builds passed twice with stable per-image inventories"
