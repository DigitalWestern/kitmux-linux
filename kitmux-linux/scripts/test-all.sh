#!/usr/bin/env bash
set -Eeuo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
workspace="$(cd -- "${script_dir}/.." && pwd)"

if [[ "${1:-}" == "--list" ]]; then
  printf '%s\n' \
    materialize-reference.sh \
    test-phase3.sh \
    test-headless.sh \
    test-standalone.sh \
    test-desktop.sh \
    test-phase4.sh \
    test-phase4-wayland.sh \
    test-phase4-persistence.sh \
    test-phase4-programs.sh \
    test-phase4-soak.sh \
    test-phase5-product.sh \
    test-phase5-navigation.sh \
    test-phase6-control.sh \
    test-phase6-resume.sh \
    test-phase6-ssh.sh \
    test-package-lifecycle.sh \
    test-phase4-clean-target.sh \
    test-clean-containers.sh
  exit 0
fi

source "${script_dir}/gate-common.sh"

run_host() {
  KITMUX_INVENTORY_VALIDATED=1 "${script_dir}/$1"
}

run_headless() {
  limactl shell kitmux-linux -- env \
    KITMUX_INVENTORY_VALIDATED=1 \
    "${workspace}/scripts/$1"
}

run_desktop() {
  limactl shell kitmux-linux-desktop -- env \
    DISPLAY="${KITMUX_DISPLAY:-:1}" \
    KITMUX_NOVNC_PORT= \
    KITMUX_INVENTORY_VALIDATED=1 \
    "${workspace}/scripts/$1"
}

run_host materialize-reference.sh
run_host test-phase3.sh
run_headless test-headless.sh
run_headless test-standalone.sh
run_desktop test-desktop.sh
run_desktop test-phase4.sh
run_desktop test-phase4-wayland.sh
run_desktop test-phase4-persistence.sh
run_desktop test-phase4-programs.sh
run_desktop test-phase4-soak.sh
run_desktop test-phase5-product.sh
KITMUX_RAPID_NAV_GATE=1 KITMUX_ACCESSIBILITY_GATE=1 run_desktop test-phase5-navigation.sh
run_desktop test-phase6-control.sh
run_desktop test-phase6-resume.sh
run_desktop test-phase6-ssh.sh
run_desktop test-package-lifecycle.sh
run_host test-phase4-clean-target.sh
run_headless test-clean-containers.sh

echo "All Kitmux Linux gates: OK"
