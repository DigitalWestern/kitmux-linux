#!/usr/bin/env bash
set -Eeuo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Run this gate inside a Linux build VM or CI runner." >&2
  exit 1
fi

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "${script_dir}/gate-common.sh"
export KITMUX_REQUIRE_DURABLE_INPUTS=1
linux_root="$(cd -- "${script_dir}/../.." && pwd)"
missing_macos="$(mktemp -d "${TMPDIR:-/tmp}/kitmux-no-macos.XXXXXX")"
build_root="$(mktemp -d "${TMPDIR:-/tmp}/kitmux-standalone-build.XXXXXX")"
cleanup() {
  rm -rf -- "${missing_macos}" "${build_root}"
}
trap cleanup EXIT

# The missing path is deliberate: this gate must fail if the private checkout
# becomes an implicit input again.
KITMUX_MACOS_REPO="${missing_macos}/does-not-exist" \
  "${script_dir}/materialize-reference.sh"
"${script_dir}/materialize-dependencies.sh"

KITMUX_BUILD_DIR="${build_root}/cmake" \
  KITMUX_BUILD_APP_RUNTIME=0 \
  "${script_dir}/test-headless.sh"

echo "Standalone Linux source and headless gate: OK"
