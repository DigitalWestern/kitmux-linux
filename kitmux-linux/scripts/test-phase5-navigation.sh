#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" || -z "${DISPLAY:-}" ]]; then
  echo "Run this gate on Linux with DISPLAY set to an existing X11 display." >&2
  exit 1
fi
for command in xdotool seq; do
  command -v "${command}" >/dev/null || {
    echo "Missing required command: ${command}" >&2
    exit 1
  }
done

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/kitmux-phase5-navigation.XXXXXX")"
runtime="${temporary_root}/runtime"
log="${temporary_root}/kitmux.log"
app_pid=""
child_pid=""

cleanup() {
  if [[ -n "${app_pid}" ]] && kill -0 "${app_pid}" 2>/dev/null; then
    kill "${app_pid}" 2>/dev/null || true
    wait "${app_pid}" 2>/dev/null || true
  fi
  if [[ -n "${child_pid}" ]] && kill -0 "${child_pid}" 2>/dev/null; then
    kill "${child_pid}" 2>/dev/null || true
  fi
  rm -rf -- "${temporary_root}" 2>/dev/null || true
}
trap cleanup EXIT

wait_for_log() { # regex, description
  local pattern="$1" description="$2"
  for _ in $(seq 1 200); do
    grep -qE "${pattern}" "${log}" 2>/dev/null && return 0
    if [[ -n "${app_pid}" ]] && ! kill -0 "${app_pid}" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  echo "Kitmux never reported ${description}; log follows:" >&2
  cat "${log}" >&2
  exit 1
}

wait_for_log_count() { # regex, count, description
  local pattern="$1" want="$2" description="$3"
  for _ in $(seq 1 200); do
    [[ "$(grep -cE "${pattern}" "${log}" 2>/dev/null || true)" -ge "${want}" ]] && return 0
    if [[ -n "${app_pid}" ]] && ! kill -0 "${app_pid}" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  echo "Kitmux never reported ${description}; log follows:" >&2
  cat "${log}" >&2
  exit 1
}

KITMUX_BUILD_APP_RUNTIME=1 "${script_dir}/build-release-runtime.sh" "${runtime}"
app="${runtime}/bin/kitmux"
mkdir -p "${temporary_root}/config" "${temporary_root}/state" \
  "${temporary_root}/data" "${temporary_root}/cache"

launch_environment=(
  env -i
  "DISPLAY=${DISPLAY}"
  "HOME=${HOME}"
  "LANG=${LANG:-C.UTF-8}"
  "PATH=/usr/bin:/bin"
  "GSK_RENDERER=gl"
  "GTK_IM_MODULE=gtk-im-context-simple"
  "KITMUX_INTERACTION_DIAGNOSTICS=1"
  "XDG_CONFIG_HOME=${temporary_root}/config"
  "XDG_STATE_HOME=${temporary_root}/state"
  "XDG_DATA_HOME=${temporary_root}/data"
  "XDG_CACHE_HOME=${temporary_root}/cache"
)
for variable in USER LOGNAME XAUTHORITY XDG_RUNTIME_DIR WAYLAND_DISPLAY GDK_BACKEND \
  WAYLAND_DEBUG KITMUX_AUTONAVIGATION KITMUX_SPLIT_GATE KITMUX_HIDDEN_SESSION_GATE \
  KITMUX_ACCESSIBILITY_GATE KITMUX_RAPID_NAV_GATE; do
  if [[ -n "${!variable:-}" ]]; then
    launch_environment+=("${variable}=${!variable}")
  fi
done

"${launch_environment[@]}" "${app}" >"${log}" 2>&1 &
app_pid=$!
wait_for_log '^kitmux event=terminal_ready pid=[0-9]+' "terminal readiness"
wait_for_log '^kitmux event=navigation_ready$' "navigation readiness"
if [[ -n "${KITMUX_ACCESSIBILITY_GATE:-}" ]]; then
  wait_for_log '^kitmux event=accessibility_ready roles=true focus=true$' \
    "native GTK roles and focus transfer"
fi
wait_for_log '^kitmux event=viewport ' "the first terminal viewport"
child_pid="$(sed -n 's/^kitmux event=terminal_ready pid=\([0-9][0-9]*\).*/\1/p' "${log}" | head -n 1)"

window_id="${KITMUX_INPUT_WINDOW_ID:-}"
for _ in $(seq 1 200); do
  [[ -n "${window_id}" ]] && break
  window_id="$(xdotool search --onlyvisible --pid "${app_pid}" 2>/dev/null | head -n 1 || true)"
  sleep 0.1
done
if [[ -z "${window_id}" ]]; then
  echo "Could not find the Kitmux window." >&2
  cat "${log}" >&2
  exit 1
fi

if [[ -z "${KITMUX_AUTONAVIGATION:-}" ]]; then
  xdotool windowactivate --sync "${window_id}"
  xdotool windowfocus --sync "${window_id}"
  focus_x=450
  focus_y=320
  xdotool mousemove --window "${window_id}" "${focus_x}" "${focus_y}"
  xdotool click 1
fi

if [[ -n "${KITMUX_RAPID_NAV_GATE:-}" ]]; then
  if [[ -z "${KITMUX_AUTONAVIGATION:-}" ]]; then
    for _ in $(seq 1 8); do xdotool key --clearmodifiers super+t; done
  fi
  wait_for_log '^kitmux event=navigation_changed workspaces=1 groups=1 tabs=9 workspace=0 group=0 tab=8$' \
    "nine terminal tabs"
  if [[ -z "${KITMUX_AUTONAVIGATION:-}" ]]; then
    for _ in $(seq 1 10); do
      for index in $(seq 1 9); do xdotool key --clearmodifiers "alt+${index}"; done
    done
    for _ in $(seq 1 8); do xdotool key --clearmodifiers super+n; done
  fi
  wait_for_log '^kitmux event=navigation_changed workspaces=9 groups=1 tabs=1 workspace=8 group=0 tab=0$' \
    "nine workspaces"
  if [[ -z "${KITMUX_AUTONAVIGATION:-}" ]]; then
    for _ in $(seq 1 10); do
      for index in $(seq 1 9); do xdotool key --clearmodifiers "super+${index}"; done
    done
  fi
  wait_for_log '^kitmux event=navigation_changed workspaces=9 groups=1 tabs=1 workspace=8 group=0 tab=0$' \
    "the final rapid-navigation target"
  wait_for_log_count '^kitmux event=navigation_changed ' 196 \
    "all rapid-navigation state transitions"
  if [[ "$(grep -c '^kitmux event=navigation_changed ' "${log}")" -lt 196 ]] \
    || grep -q '^kitmux event=navigation_rejected$' "${log}"; then
    echo "Rapid navigation dropped or rejected a state transition." >&2
    cat "${log}" >&2
    exit 1
  fi
elif [[ -n "${KITMUX_HIDDEN_SESSION_GATE:-}" ]]; then
  [[ -n "${KITMUX_AUTONAVIGATION:-}" ]] || xdotool key --clearmodifiers super+t
  wait_for_log '^kitmux event=navigation_changed workspaces=1 groups=1 tabs=2 workspace=0 group=0 tab=1$' \
    "the hidden-session test tab"
  xdotool mousemove --window "${window_id}" 450 320
  xdotool click 1
  xdotool type --clearmodifiers --delay 1 -- \
    'i=0; while [ $i -lt 300 ]; do echo hidden-$i; i=$((i+1)); sleep 0.01; done'
  xdotool key --clearmodifiers Return
  [[ -n "${KITMUX_AUTONAVIGATION:-}" ]] || xdotool key --clearmodifiers alt+1
  wait_for_log '^kitmux event=navigation_changed workspaces=1 groups=1 tabs=2 workspace=0 group=0 tab=0$' \
    "selection away from the output session"
  wait_for_log '^kitmux event=hidden_session_pumped surface=.* bytes=[1-9][0-9]*$' \
    "fair hidden-session PTY pumping"
elif [[ -n "${KITMUX_SPLIT_GATE:-}" ]]; then
  [[ -n "${KITMUX_AUTONAVIGATION:-}" ]] || xdotool key --clearmodifiers super+d
  wait_for_log '^kitmux event=split_changed panes=2 ' "the first split"
  [[ -n "${KITMUX_AUTONAVIGATION:-}" ]] || xdotool key --clearmodifiers super+shift+d
  wait_for_log '^kitmux event=split_changed panes=3 ' "the nested split"
  [[ -n "${KITMUX_AUTONAVIGATION:-}" ]] || xdotool key --clearmodifiers super+shift+p
  for _ in $(seq 1 200); do
    [[ "$(grep -c '^kitmux event=split_changed panes=3 ' "${log}" || true)" -ge 2 ]] && break
    sleep 0.1
  done
  if [[ "$(grep -c '^kitmux event=split_changed panes=3 ' "${log}" || true)" -lt 2 ]]; then
    echo "Kitmux did not cycle split focus." >&2
    cat "${log}" >&2
    exit 1
  fi
  if [[ -z "${KITMUX_AUTONAVIGATION:-}" ]]; then
    local_x="$(sed -n 's/^kitmux event=pointer_press .* x=\([0-9][0-9]*\)\..*/\1/p' "${log}" | head -n 1)"
    local_y="$(sed -n 's/^kitmux event=pointer_press .* y=\([0-9][0-9]*\)\..*/\1/p' "${log}" | head -n 1)"
    viewport_width="$(sed -n 's/^kitmux event=viewport width=\([0-9][0-9]*\).*/\1/p' "${log}" | tail -n 1)"
    viewport_height="$(sed -n 's/^kitmux event=viewport .* height=\([0-9][0-9]*\).*/\1/p' "${log}" | tail -n 1)"
    area_x="$((focus_x - local_x))"
    area_y="$((focus_y - local_y))"
    divider_x="$((area_x + viewport_width / 2))"
    divider_y="$((area_y + viewport_height * 3 / 4))"
    xdotool mousemove --window "${window_id}" "${divider_x}" "${divider_y}"
    sleep 0.2
    xdotool mousedown 1
    sleep 0.2
    xdotool mousemove --window "${window_id}" "$((divider_x + 80))" "${divider_y}"
    sleep 0.2
    xdotool mouseup 1
    wait_for_log '^kitmux event=divider_resized ' "divider drag resizing"
    xdotool key --clearmodifiers super+shift+h
    wait_for_log '^kitmux event=pane_resized direction=left$' "keyboard split resizing"
    xdotool mousemove --window "${window_id}" "$((area_x + viewport_width / 4))" "${divider_y}"
    xdotool click 1
    wait_for_log '^kitmux event=pane_focused source=pointer$' "pointer pane focus"
  else
    wait_for_log '^kitmux event=pane_resized direction=left$' "keyboard split resizing"
  fi
  wait_for_log '^kitmux event=viewport .* panes=3$' "three rendered terminal regions"
else
[[ -n "${KITMUX_AUTONAVIGATION:-}" ]] || xdotool key --clearmodifiers super+n
wait_for_log '^kitmux event=navigation_changed workspaces=2 groups=1 tabs=1 workspace=1 group=0 tab=0$' \
  "workspace creation"
[[ -n "${KITMUX_AUTONAVIGATION:-}" ]] || xdotool key --clearmodifiers super+1
wait_for_log '^kitmux event=navigation_changed workspaces=2 groups=1 tabs=1 workspace=0 group=0 tab=0$' \
  "workspace number selection"
[[ -n "${KITMUX_AUTONAVIGATION:-}" ]] || xdotool key --clearmodifiers super+t
wait_for_log '^kitmux event=navigation_changed workspaces=2 groups=1 tabs=2 workspace=0 group=0 tab=1$' \
  "tab creation"
[[ -n "${KITMUX_AUTONAVIGATION:-}" ]] || xdotool key --clearmodifiers alt+1
wait_for_log '^kitmux event=navigation_changed workspaces=2 groups=1 tabs=2 workspace=0 group=0 tab=0$' \
  "tab number selection"
[[ -n "${KITMUX_AUTONAVIGATION:-}" ]] || xdotool key --clearmodifiers super+alt+t
wait_for_log '^kitmux event=navigation_changed workspaces=2 groups=2 tabs=1 workspace=0 group=1 tab=0$' \
  "group creation"
[[ -n "${KITMUX_AUTONAVIGATION:-}" ]] || xdotool key --clearmodifiers super+shift+bracketleft
wait_for_log '^kitmux event=navigation_changed workspaces=2 groups=2 tabs=2 workspace=0 group=0 tab=0$' \
  "previous group selection"
[[ -n "${KITMUX_AUTONAVIGATION:-}" ]] || xdotool key --clearmodifiers super+shift+bracketright
wait_for_log '^kitmux event=navigation_changed workspaces=2 groups=2 tabs=1 workspace=0 group=1 tab=0$' \
  "next group selection"
fi

geometry="$(xdotool getwindowgeometry --shell "${window_id}")"
window_x="$(awk -F= '$1 == "X" {print $2}' <<<"${geometry}")"
window_y="$(awk -F= '$1 == "Y" {print $2}' <<<"${geometry}")"
window_width="$(awk -F= '$1 == "WIDTH" {print $2}' <<<"${geometry}")"
window_height="$(awk -F= '$1 == "HEIGHT" {print $2}' <<<"${geometry}")"
xdotool mousemove --sync "$((window_x + window_width + 100))" \
  "$((window_y + window_height / 2))"
sleep 0.2
xdotool mousemove --sync "$((window_x + 800))" "$((window_y + 400))"
sleep 0.2
xdotool click 1
xdotool type --clearmodifiers --delay 20 -- exit
xdotool key --clearmodifiers Return
for _ in $(seq 1 200); do
  ! kill -0 "${app_pid}" 2>/dev/null && break
  sleep 0.1
done
if kill -0 "${app_pid}" 2>/dev/null; then
  echo "Kitmux did not exit after navigation testing." >&2
  cat "${log}" >&2
  exit 1
fi
if ! wait "${app_pid}"; then
  echo "Kitmux exited unsuccessfully after navigation testing." >&2
  cat "${log}" >&2
  exit 1
fi
expected_created=3
expected_sessions=4
gate_name="workspace, group, and tab navigation"
if [[ -n "${KITMUX_SPLIT_GATE:-}" ]]; then
  expected_created=2
  expected_sessions=3
  gate_name="nested split and session ownership"
elif [[ -n "${KITMUX_HIDDEN_SESSION_GATE:-}" ]]; then
  expected_created=1
  expected_sessions=2
  gate_name="hidden-session fairness and active input ownership"
elif [[ -n "${KITMUX_RAPID_NAV_GATE:-}" ]]; then
  expected_created=16
  expected_sessions=17
  gate_name="rapid navigation and permanent-session churn"
fi
if [[ "$(grep -c '^kitmux event=terminal_surface_created ' "${log}")" != "${expected_created}" ]] \
  || ! grep -qE "^kitmux event=shutdown .* sessions=${expected_sessions} reaped=true$" "${log}"; then
  echo "Kitmux did not create and reap every permanent surface session." >&2
  cat "${log}" >&2
  exit 1
fi
app_pid=""
child_pid=""

echo "Phase 5 ${gate_name} gate: OK"
