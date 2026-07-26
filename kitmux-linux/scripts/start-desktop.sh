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

if ! tigervncserver -list 2>/dev/null \
    | awk -v display="${display_number}" \
      '$1 == display || $1 == ":" display { found = 1 } END { exit !found }'; then
  tigervncserver ":${display_number}" \
    -localhost yes \
    -SecurityTypes None \
    -geometry "${geometry}" \
    -depth 24 \
    -desktop "Kitmux Linux development" \
    -xstartup "${xstartup}"
fi

# GTK/WebKit consult the desktop portal even in this disposable VNC harness.
# User services do not inherit the display created after login, so publish it
# before activating the portal backend. Without this, each GTK host waits for
# the portal's D-Bus timeout and test results depend on stale session state.
export DISPLAY=":${display_number}"
export XDG_CURRENT_DESKTOP="${XDG_CURRENT_DESKTOP:-XFCE}"
dbus-update-activation-environment --systemd DISPLAY XDG_CURRENT_DESKTOP
systemctl --user reset-failed \
  xdg-desktop-portal.service xdg-desktop-portal-gtk.service
systemctl --user restart \
  xdg-desktop-portal-gtk.service xdg-desktop-portal.service

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
