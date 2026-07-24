#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Run this script inside the Ubuntu development VM." >&2
  exit 1
fi

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
xstartup="${script_dir}/../desktop/xfce-xstartup"
display_number="${KITMUX_VNC_DISPLAY:-1}"
geometry="${KITMUX_VNC_GEOMETRY:-1440x900}"
novnc_port="${KITMUX_NOVNC_PORT:-6080}"
runtime_dir="${XDG_RUNTIME_DIR:-/tmp/kitmux-desktop-${UID}}"
novnc_pid_file="${runtime_dir}/novnc.pid"
novnc_log="${runtime_dir}/novnc.log"

mkdir -p "${runtime_dir}"
chmod 700 "${runtime_dir}"

if ! tigervncserver -list 2>/dev/null | grep -Eq ":${display_number}([[:space:]]|$)"; then
  tigervncserver ":${display_number}" \
    -localhost yes \
    -SecurityTypes None \
    -geometry "${geometry}" \
    -depth 24 \
    -desktop "Kitmux Linux development" \
    -xstartup "${xstartup}"
fi

if [[ -f "${novnc_pid_file}" ]] && kill -0 "$(cat "${novnc_pid_file}")" 2>/dev/null; then
  :
else
  nohup websockify \
    --web /usr/share/novnc \
    --file-only \
    --log-file "${novnc_log}" \
    "127.0.0.1:${novnc_port}" \
    "127.0.0.1:$((5900 + display_number))" \
    </dev/null >/dev/null 2>&1 &
  echo "$!" >"${novnc_pid_file}"
fi

for _ in $(seq 1 50); do
  if curl --fail --silent "http://127.0.0.1:${novnc_port}/vnc.html" >/dev/null; then
    echo "Desktop ready on DISPLAY=:${display_number}"
    echo "noVNC ready on guest http://127.0.0.1:${novnc_port}/vnc.html"
    exit 0
  fi
  sleep 0.1
done

echo "noVNC did not become ready; inspect ${novnc_log}" >&2
exit 1
