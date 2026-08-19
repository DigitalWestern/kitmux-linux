#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" || -z "${DISPLAY:-}" ]]; then
  echo "Run this gate on Linux with DISPLAY set to an existing X11 display." >&2
  exit 1
fi
for command in weston xdotool; do
  command -v "${command}" >/dev/null || {
    echo "Missing required command: ${command}" >&2
    exit 1
  }
done

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "${script_dir}/gate-common.sh"
gate_script="${KITMUX_WAYLAND_GATE:-test-phase4.sh}"
gate_label="${KITMUX_WAYLAND_LABEL:-Phase 4 native-Wayland product interaction gate}"
if [[ "${gate_script}" == */* || ! -x "${script_dir}/${gate_script}" ]]; then
  echo "Invalid native-Wayland gate: ${gate_script}" >&2
  exit 1
fi
runtime_dir="$(mktemp -d /tmp/kitmux-phase4-wayland.XXXXXX)"
weston_log="${runtime_dir}/weston.log"
wayland_socket="wayland-kitmux-phase4"
weston_pid=""

cleanup() {
  if [[ -n "${weston_pid}" ]] && kill -0 "${weston_pid}" 2>/dev/null; then
    kill "${weston_pid}" 2>/dev/null || true
    wait "${weston_pid}" 2>/dev/null || true
  fi
  if command -v fusermount3 >/dev/null && mountpoint -q "${runtime_dir}/doc" 2>/dev/null; then
    fusermount3 -u "${runtime_dir}/doc" 2>/dev/null || true
  fi
  rm -rf -- "${runtime_dir}" 2>/dev/null || true
}
trap cleanup EXIT
chmod 700 "${runtime_dir}"

env DISPLAY="${DISPLAY}" XDG_RUNTIME_DIR="${runtime_dir}" \
  weston --backend=x11 --renderer=gl --width=1024 --height=700 \
    --socket="${wayland_socket}" --shell=kiosk --no-config --idle-time=0 \
    --log="${weston_log}" >/dev/null 2>&1 &
weston_pid=$!

window_id=""
for _ in $(seq 1 120); do
  if ! kill -0 "${weston_pid}" 2>/dev/null; then
    cat "${weston_log}" >&2
    exit 1
  fi
  if [[ -S "${runtime_dir}/${wayland_socket}" ]]; then
    window_id="$(
      sed -n 's/.*window id \([0-9][0-9]*\)$/\1/p' "${weston_log}" | tail -n 1
    )"
    [[ -n "${window_id}" ]] && break
  fi
  sleep 0.1
done
if [[ -z "${window_id}" ]]; then
  echo "Weston did not report a ready nested output." >&2
  cat "${weston_log}" >&2
  exit 1
fi

if ! env DISPLAY="${DISPLAY}" XDG_RUNTIME_DIR="${runtime_dir}" \
  WAYLAND_DISPLAY="${wayland_socket}" GDK_BACKEND=wayland \
  KITMUX_PHASE4_BACKEND=wayland KITMUX_INPUT_WINDOW_ID="${window_id}" \
  "${script_dir}/${gate_script}"; then
  echo "Nested Weston log follows:" >&2
  cat "${weston_log}" >&2
  exit 1
fi

echo "${gate_label}: OK"
