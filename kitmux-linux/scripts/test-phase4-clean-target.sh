#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
workspace="$(cd -- "${script_dir}/.." && pwd)"
linux_root="$(cd -- "${workspace}/.." && pwd)"
source "${script_dir}/gate-common.sh"

if [[ "${1:-}" == "--inside" ]]; then
  runtime="$(realpath "${2:?missing release runtime}")"
  command -v podman >/dev/null || {
    echo "Podman is required in the clean-gate VM." >&2
    exit 1
  }
  ubuntu_digest="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["container_images"]["docker.io/library/ubuntu:26.04"])' "${linux_root}/source-lock.json")"
  grep -Fxq "FROM docker.io/library/ubuntu@${ubuntu_digest}" \
    "${workspace}/ci/Containerfile.runtime-ubuntu"
  podman build --pull=missing \
    -t localhost/kitmux-phase4-runtime:ubuntu-26.04 \
    -f "${workspace}/ci/Containerfile.runtime-ubuntu" "${workspace}/ci"
  podman run --rm --user 0 \
    -v "${runtime}:/runtime:ro" \
    localhost/kitmux-phase4-runtime:ubuntu-26.04 \
    bash -euo pipefail -c '
      for forbidden in cc gcc clang cargo rustc cmake make pkg-config; do
        if command -v "${forbidden}" >/dev/null; then
          echo "Clean target unexpectedly has ${forbidden}." >&2
          exit 1
        fi
      done
      for forbidden_package in build-essential libgtk-4-dev libglib2.0-dev libepoxy-dev; do
        if dpkg-query -W -f="\${Status}" "${forbidden_package}" 2>/dev/null \
          | grep -q "ok installed"; then
          echo "Clean target unexpectedly has ${forbidden_package}." >&2
          exit 1
        fi
      done
      if ldd /runtime/bin/kitmux | grep -q "not found"; then
        ldd /runtime/bin/kitmux >&2
        exit 1
      fi
      test_root="$(mktemp -d /tmp/kitmux-clean-target.XXXXXX)"
      cleanup() {
        if [[ -n "${app_pid:-}" ]] && kill -0 "${app_pid}" 2>/dev/null; then
          kill "${app_pid}" 2>/dev/null || true
          wait "${app_pid}" 2>/dev/null || true
        fi
        rm -rf -- "${test_root}"
      }
      trap cleanup EXIT
      mkdir -m 700 "${test_root}/config" "${test_root}/state" \
        "${test_root}/data" "${test_root}/cache"
      xvfb-run -a env HOME=/root LANG=C.UTF-8 PATH=/usr/bin:/bin \
        GDK_BACKEND=x11 GSK_RENDERER=gl \
        XDG_CONFIG_HOME="${test_root}/config" \
        XDG_STATE_HOME="${test_root}/state" \
        XDG_DATA_HOME="${test_root}/data" \
        XDG_CACHE_HOME="${test_root}/cache" \
        /runtime/bin/kitmux >"${test_root}/kitmux.log" 2>&1 &
      app_pid=$!
      child_pid=""
      for _ in $(seq 1 200); do
        child_pid="$(sed -n "s/^kitmux event=terminal_ready pid=\([0-9][0-9]*\).*/\1/p" \
          "${test_root}/kitmux.log" | head -n 1)"
        [[ -n "${child_pid}" ]] && break
        kill -0 "${app_pid}" 2>/dev/null || break
        sleep 0.1
      done
      if [[ -z "${child_pid}" ]] || ! kill -0 "${child_pid}" 2>/dev/null; then
        echo "Release app did not launch on the no-SDK target." >&2
        cat "${test_root}/kitmux.log" >&2
        exit 1
      fi
      expected_shell="$(getent passwd "$(id -u)" | cut -d: -f7)"
      [[ "$(realpath "/proc/${child_pid}/exe")" == "$(realpath "${expected_shell}")" ]]
      grep -q "backend=GdkX11Display" "${test_root}/kitmux.log"
      kill -HUP "${child_pid}"
      for _ in $(seq 1 200); do
        ! kill -0 "${app_pid}" 2>/dev/null && break
        sleep 0.1
      done
      if kill -0 "${app_pid}" 2>/dev/null; then
        echo "Release app did not close after its clean-target shell exited." >&2
        cat "${test_root}/kitmux.log" >&2
        exit 1
      fi
      wait "${app_pid}"
      app_pid=""
      ! kill -0 "${child_pid}" 2>/dev/null
      [[ "$(stat -c "%a" "${test_root}/state/kitmux/state.json")" == 600 ]]
      echo "Phase 4 clean no-SDK release launch: OK"
    '
  exit 0
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Run this wrapper on macOS, or pass --inside in the headless Linux VM." >&2
  exit 1
fi
for command in limactl mktemp; do
  command -v "${command}" >/dev/null || {
    echo "Missing required command: ${command}" >&2
    exit 1
  }
done

shared_root="$(mktemp -d "${linux_root}/.phase4-clean-target.XXXXXX")"
cleanup() {
  rm -rf -- "${shared_root}"
}
trap cleanup EXIT
runtime="${shared_root}/runtime"

limactl shell kitmux-linux-desktop -- env KITMUX_BUILD_APP_RUNTIME=1 KITMUX_APP_TEST_HOOKS=ON \
  "${script_dir}/build-release-runtime.sh" "${runtime}"
limactl shell kitmux-linux -- \
  "${script_dir}/test-phase4-clean-target.sh" --inside "${runtime}"
