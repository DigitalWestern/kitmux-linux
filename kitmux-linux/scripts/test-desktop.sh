#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Run this script inside the Ubuntu development VM." >&2
  exit 1
fi

display_number="${KITMUX_VNC_DISPLAY:-1}"
novnc_port="${KITMUX_NOVNC_PORT:-6080}"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source_root="$(cd -- "${script_dir}/.." && pwd)"
smoke_binary="${source_root}/build/gtk4_gl_smoke"
export DISPLAY=":${display_number}"

xdpyinfo >/dev/null
curl --fail --silent "http://127.0.0.1:${novnc_port}/vnc.html" >/dev/null

mkdir -p "${source_root}/build"
cc -Wall -Wextra -Werror \
  "${source_root}/tests/gtk4_gl_smoke.c" \
  -o "${smoke_binary}" \
  $(pkg-config --cflags --libs gtk4 epoxy)

gtk_version="$(pkg-config --modversion gtk4)"
renderer="$(glxinfo -B | awk -F: '/OpenGL renderer string/ {sub(/^[[:space:]]+/, "", $2); print $2}')"

if [[ -z "${renderer}" ]]; then
  echo "Could not identify an OpenGL renderer." >&2
  exit 1
fi

echo "GTK ${gtk_version}"
echo "OpenGL renderer: ${renderer}"
echo "X11 DISPLAY=${DISPLAY} and noVNC are responding"
echo "GTK/OpenGL smoke binary: ${smoke_binary}"
