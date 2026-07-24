#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Run this script inside the Ubuntu development VM." >&2
  exit 1
fi

display_number="${KITMUX_VNC_DISPLAY:-1}"
runtime_dir="${XDG_RUNTIME_DIR:-/tmp/kitmux-desktop-${UID}}"
novnc_pid_file="${runtime_dir}/novnc.pid"

if [[ -f "${novnc_pid_file}" ]]; then
  novnc_pid="$(cat "${novnc_pid_file}")"
  if kill -0 "${novnc_pid}" 2>/dev/null; then
    kill "${novnc_pid}"
  fi
  rm -f "${novnc_pid_file}"
fi

if tigervncserver -list 2>/dev/null | grep -Eq ":${display_number}([[:space:]]|$)"; then
  tigervncserver -kill ":${display_number}"
fi
