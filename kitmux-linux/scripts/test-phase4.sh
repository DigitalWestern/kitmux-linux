#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Run this gate on Linux." >&2
  exit 1
fi
if [[ -z "${DISPLAY:-}" ]]; then
  echo "Set DISPLAY to an existing X11 display." >&2
  exit 1
fi
backend="${KITMUX_PHASE4_BACKEND:-x11}"
if [[ "${backend}" != "x11" && "${backend}" != "wayland" ]]; then
  echo "KITMUX_PHASE4_BACKEND must be x11 or wayland." >&2
  exit 1
fi
mouse_reader_seconds=4
if [[ "${backend}" == "x11" ]]; then
  mouse_reader_seconds=120
fi
clipboard_commands=()
if [[ "${backend}" == "x11" ]]; then
  clipboard_commands=(xsel)
else
  clipboard_commands=(wl-copy wl-paste)
fi
for command in install python3 readelf ldd realpath seq stat xdotool xdpyinfo "${clipboard_commands[@]}"; do
  command -v "${command}" >/dev/null || {
    echo "Missing required command: ${command}" >&2
    exit 1
  }
done
xdpyinfo >/dev/null

clipboard_set() {
  if [[ "${backend}" == "x11" ]]; then
    xsel --clipboard --input
  else
    wl-copy
  fi
}

clipboard_get() {
  if [[ "${backend}" == "x11" ]]; then
    xsel --clipboard --output
  else
    wl-paste --no-newline
  fi
}

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
workspace="$(cd -- "${script_dir}/.." && pwd)"
linux_root="$(cd -- "${workspace}/.." && pwd)"
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/kitmux-phase4.XXXXXX")"
runtime="${temporary_root}/runtime"
log="${temporary_root}/kitmux.log"
command_dir="${temporary_root}/cwd"
command_marker="${command_dir}/gate-command-ran"
control_marker="${command_dir}/control-c-ran"
safe_paste_marker="${command_dir}/safe-paste-ran"
cancelled_paste_marker="${command_dir}/cancelled-paste-ran"
confirmed_paste_marker="${command_dir}/confirmed-paste-ran"
mouse_marker="${command_dir}/mouse-test-ran"
pointer_flood_marker="${command_dir}/pointer-flood-ran"
config_root="${temporary_root}/config"
state_root="${temporary_root}/state"
data_root="${temporary_root}/data"
cache_root="${temporary_root}/cache"
settings_path="${config_root}/kitmux/settings.json"
state_path="${state_root}/kitmux/state.json"
expected_title="kitmux-phase4-title"
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
  if [[ -d "${temporary_root}" ]]; then
    rm -rf -- "${temporary_root}"
  fi
}
trap cleanup EXIT

wait_for_log() { # regex, description
  local pattern="$1" description="$2"
  for _ in $(seq 1 200); do
    if grep -qE "${pattern}" "${log}" 2>/dev/null; then
      return 0
    fi
    if [[ -n "${app_pid}" ]] && ! kill -0 "${app_pid}" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  echo "Kitmux never reported ${description}; log follows:" >&2
  cat "${log}" >&2
  exit 1
}

wait_for_file() { # path, description
  local path="$1" description="$2"
  for _ in $(seq 1 200); do
    [[ -e "${path}" ]] && return 0
    sleep 0.1
  done
  echo "Timed out waiting for ${description}." >&2
  cat "${log}" >&2
  exit 1
}

KITMUX_BUILD_APP_RUNTIME=1 KITMUX_APP_TEST_HOOKS=ON \
  "${script_dir}/build-release-runtime.sh" "${runtime}"

app="${runtime}/bin/kitmux"
kitty="${runtime}/lib/app/libkitty.so"
if [[ ! -x "${app}" || ! -f "${kitty}" ]]; then
  echo "Phase 4 runtime is missing bin/kitmux or isolated lib/app/libkitty.so." >&2
  exit 1
fi
grep -Fq 'Library runpath: [$ORIGIN/../lib/app]' < <(readelf -d "${app}")
grep -Fq 'Library runpath: [$ORIGIN:$ORIGIN/..]' < <(readelf -d "${kitty}")

loader_closure="$(env -u LD_LIBRARY_PATH ldd "${app}")"
if grep -q 'not found' <<<"${loader_closure}"; then
  echo "Phase 4 runtime has an unresolved library:" >&2
  grep 'not found' <<<"${loader_closure}" >&2
  exit 1
fi
resolved_kitty="$(awk '$1 == "libkitty.so" {print $3; exit}' <<<"${loader_closure}")"
resolved_python="$(
  awk '$1 ~ /^libpython3[.].*[.]so[.]1[.]0$/ {print $3; exit}' \
    <<<"${loader_closure}"
)"
if [[ -z "${resolved_kitty}" || -z "${resolved_python}" ]] \
    || [[ "$(realpath "${resolved_kitty}")" != "$(realpath "${kitty}")" ]] \
    || [[ "$(realpath "${resolved_python}")" != "${runtime}/lib/app/"* ]]; then
  echo "Phase 4 runtime did not resolve libkitty and libpython from its isolated app directory." >&2
  exit 1
fi
if grep -Fq "${linux_root}" <<<"${loader_closure}" \
    || readelf -d "${app}" "${kitty}" | grep -Fq "${linux_root}"; then
  echo "Phase 4 runtime leaked a developer checkout path." >&2
  exit 1
fi

mkdir -p "${command_dir}"
install -d -m 700 "${config_root}/kitmux" "${state_root}/kitmux" \
  "${data_root}" "${cache_root}"
printf '%s\n' \
  '{"version":1,"confirmCloseWithRunningProcess":true,"gateSentinel":"kept","linuxShortcutBindings":{"terminal.find":{"key":"g","control":true,"shift":true}}}' \
  >"${settings_path}"
chmod 600 "${settings_path}"
launch_environment=(
  env -i
  "DISPLAY=${DISPLAY}"
  "HOME=${HOME}"
  "LANG=${LANG:-C.UTF-8}"
  "PATH=/usr/bin:/bin"
  "GSK_RENDERER=gl"
  "GTK_IM_MODULE=gtk-im-context-simple"
  "KITMUX_PHASE4_CWD=${command_dir}"
  "KITMUX_AUTOPASTE=cancel-first"
  "KITMUX_AUTOCLOSE=cancel-first"
  "KITMUX_INTERACTION_DIAGNOSTICS=1"
  "KITMUX_SECRET_CANARY=must-not-appear-in-diagnostics"
  "XDG_CONFIG_HOME=${config_root}"
  "XDG_STATE_HOME=${state_root}"
  "XDG_DATA_HOME=${data_root}"
  "XDG_CACHE_HOME=${cache_root}"
)
for variable in USER LOGNAME XAUTHORITY XDG_RUNTIME_DIR; do
  if [[ -n "${!variable:-}" ]]; then
    launch_environment+=("${variable}=${!variable}")
  fi
done
for variable in WAYLAND_DISPLAY GDK_BACKEND WAYLAND_DEBUG; do
  if [[ -n "${!variable:-}" ]]; then
    launch_environment+=("${variable}=${!variable}")
  fi
done

"${launch_environment[@]}" "${app}" >"${log}" 2>&1 &
app_pid=$!
wait_for_log '^kitmux event=terminal_ready pid=[0-9]+' "terminal readiness"
wait_for_log '^kitmux event=navigation_ready$' "navigation readiness"
wait_for_log '^kitmux event=viewport ' "the first terminal viewport"
if [[ "${backend}" == "wayland" ]]; then
  wait_for_log '^kitmux event=terminal_ready .* backend=GdkWaylandDisplay$' \
    "the native Wayland product backend"
else
  wait_for_log '^kitmux event=terminal_ready .* backend=GdkX11Display$' \
    "the X11 product backend"
fi
child_pid="$(
  sed -n 's/^kitmux event=terminal_ready pid=\([0-9][0-9]*\).*/\1/p' \
    "${log}" | head -n 1
)"
if [[ -z "${child_pid}" ]] || ! kill -0 "${child_pid}" 2>/dev/null; then
  echo "Kitmux did not launch a live shell child." >&2
  exit 1
fi
expected_shell="$(getent passwd "$(id -u)" | cut -d: -f7)"
if [[ "$(realpath "/proc/${child_pid}/exe")" != "$(realpath "${expected_shell}")" ]]; then
  echo "Kitmux did not launch the account shell from passwd." >&2
  exit 1
fi

window_id="${KITMUX_INPUT_WINDOW_ID:-}"
for _ in $(seq 1 200); do
  [[ -n "${window_id}" ]] && break
  largest_area=0
  for candidate in $(xdotool search --onlyvisible --pid "${app_pid}" 2>/dev/null || true); do
    geometry="$(xdotool getwindowgeometry --shell "${candidate}" 2>/dev/null || true)"
    width="$(awk -F= '$1 == "WIDTH" {print $2}' <<<"${geometry}")"
    height="$(awk -F= '$1 == "HEIGHT" {print $2}' <<<"${geometry}")"
    area=$(( ${width:-0} * ${height:-0} ))
    if (( area > largest_area )); then
      largest_area="${area}"
      window_id="${candidate}"
    fi
  done
  [[ -n "${window_id}" ]] && break
  sleep 0.1
done
if [[ -z "${window_id}" ]]; then
  echo "Could not find the Kitmux window." >&2
  cat "${log}" >&2
  exit 1
fi
window_geometry="$(xdotool getwindowgeometry --shell "${window_id}")"
window_x="$(awk -F= '$1 == "X" {print $2}' <<<"${window_geometry}")"
window_y="$(awk -F= '$1 == "Y" {print $2}' <<<"${window_geometry}")"
window_width="$(awk -F= '$1 == "WIDTH" {print $2}' <<<"${window_geometry}")"
window_height="$(awk -F= '$1 == "HEIGHT" {print $2}' <<<"${window_geometry}")"
move_in_window() { # x, y
  xdotool mousemove --sync "$((window_x + $1))" "$((window_y + $2))"
}

xdotool windowactivate --sync "${window_id}"
xdotool windowfocus --sync "${window_id}"
sleep 0.2
xdotool mousemove --sync "$((window_x + window_width + 100))" \
  "$((window_y + window_height / 2))"
sleep 0.2
move_in_window 800 400
sleep 0.2
xdotool click 1
wait_for_log '^kitmux event=pointer_press button=1( |$)' "terminal pointer focus"
shell_command="printf '\\033]0;${expected_title}\\007'; sleep 0.2; cd \"\$KITMUX_PHASE4_CWD\"; seq 1 50000; printf 'phase4-output-complete\\n' > gate-command-ran"
xdotool type --clearmodifiers --delay 1 -- "${shell_command}"
xdotool key --clearmodifiers Return

for size in "760 480" "940 620" "800 520" "920 600" "780 500" "900 580"; do
  read -r width height <<<"${size}"
  xdotool windowsize --sync "${window_id}" "${width}" "${height}"
  sleep 0.1
done

wait_for_file "${command_marker}" "the driven shell command"
wait_for_log '^kitmux event=title_updated characters=19$' "the title update"
wait_for_log '^kitmux event=cwd_updated valid=true$' "the cwd update"
if [[ "$(grep -c '^kitmux event=viewport ' "${log}" || true)" -lt 4 ]]; then
  echo "Kitmux did not render enough live resizes during output flood." >&2
  cat "${log}" >&2
  exit 1
fi

# Plain Control-C belongs to the terminal, while app shortcuts are consumed
# before the input method. Interrupting sleep proves the terminal path stayed
# reachable.
xdotool type --clearmodifiers --delay 1 -- "sleep 20"
xdotool key --clearmodifiers Return
sleep 0.3
xdotool key --clearmodifiers ctrl+c
xdotool type --clearmodifiers --delay 1 -- \
  "printf control-c-ok > \"${control_marker}\""
xdotool key --clearmodifiers Return
wait_for_file "${control_marker}" "plain Control-C terminal routing"

# GTK clipboard reads are asynchronous. Safe text reaches libkitty directly;
# the first unsafe paste is cancelled and the second is confirmed by the
# smoke-only deterministic dialog driver. Keep a PTY writer active throughout
# so either display backend would expose a synchronous clipboard deadlock.
clipboard_flood_marker="${command_dir}/clipboard-flood-ran"
xdotool type --clearmodifiers --delay 1 -- \
  "(i=0; while [ \$i -lt 1000 ]; do printf 'clipboard-flood-%04d\\n' \"\$i\"; i=\$((i + 1)); sleep 0.002; done; printf clipboard-flood-ok > \"${clipboard_flood_marker}\") &"
xdotool key --clearmodifiers Return
printf '%s' "printf safe-paste-ok > \"${safe_paste_marker}\"" \
  | clipboard_set
xdotool key --clearmodifiers ctrl+shift+v
wait_for_log '^kitmux event=paste bytes=[1-9][0-9]*$' "safe clipboard paste"
xdotool key --clearmodifiers Return
wait_for_file "${safe_paste_marker}" "safe clipboard round trip"

cancelled_payload="${temporary_root}/cancelled-paste.txt"
{
  printf 'touch "%s"; #' "${cancelled_paste_marker}"
  head -c 9000 /dev/zero | tr '\0' a
  printf '\n'
} >"${cancelled_payload}"
clipboard_set <"${cancelled_payload}"
xdotool key --clearmodifiers ctrl+shift+v
wait_for_log '^kitmux event=paste_cancelled reason=large$' "cancelled unsafe paste"
sleep 0.2
if [[ -e "${cancelled_paste_marker}" ]]; then
  echo "Cancelled unsafe paste reached the shell." >&2
  exit 1
fi

confirmed_payload="${temporary_root}/confirmed-paste.txt"
{
  printf 'printf confirmed-paste-ok > "%s"; #' "${confirmed_paste_marker}"
  head -c 9000 /dev/zero | tr '\0' b
  printf '\n'
} >"${confirmed_payload}"
clipboard_set <"${confirmed_payload}"
xdotool key --clearmodifiers ctrl+shift+v
wait_for_log '^kitmux event=paste bytes=9[0-9][0-9][0-9]$' "confirmed unsafe paste"
xdotool key --clearmodifiers Return
wait_for_file "${confirmed_paste_marker}" "confirmed unsafe paste execution"
wait_for_file "${clipboard_flood_marker}" "PTY progress during clipboard activity"

# Search and font controls use the production shortcut path. Search is native
# libkitty state; Escape cancels it without sending the key to the shell.
xdotool type --clearmodifiers --delay 1 -- \
  "printf 'phase4-search-token\\nphase4-search-token\\n'"
xdotool key --clearmodifiers Return
sleep 0.2
xdotool key --clearmodifiers ctrl+shift+g
sleep 0.2
xdotool type --clearmodifiers --delay 20 -- "phase4-search-token"
wait_for_log '^kitmux event=search_updated matches=[1-9][0-9]*$' "terminal search matches"
xdotool key --clearmodifiers Return
wait_for_log '^kitmux event=search_navigated backwards=false found=true$' \
  "terminal search navigation"
xdotool key --clearmodifiers Escape
wait_for_log '^kitmux event=search_cleared$' "terminal search marker clearing"
xdotool key --clearmodifiers ctrl+shift+equal
xdotool key --clearmodifiers ctrl+minus
xdotool key --clearmodifiers ctrl+0
wait_for_log '^kitmux event=font_size points=' "font controls"

# A word selection copied through GTK must be observable by a separate X11
# clipboard client. Derive the terminal origin from its latest viewport so
# responsive navigation rows do not turn this into a fixed-coordinate test.
xdotool type --clearmodifiers --delay 1 -- \
  "printf '\\033[2J\\033[Hkitmuxcopytoken\\nkitmuxcopytoken\\nkitmuxcopytoken\\nkitmuxcopytoken\\nkitmuxcopytoken\\nkitmuxcopytoken\\n'"
xdotool key --clearmodifiers Return
sleep 0.3
window_geometry="$(xdotool getwindowgeometry --shell "${window_id}")"
current_width="$(awk -F= '$1 == "WIDTH" {print $2}' <<<"${window_geometry}")"
current_height="$(awk -F= '$1 == "HEIGHT" {print $2}' <<<"${window_geometry}")"
viewport_line="$(grep '^kitmux event=viewport ' "${log}" | tail -n 1)"
viewport_width="$(awk '{for (i = 1; i <= NF; i++) if ($i ~ /^width=/) {sub(/^width=/, "", $i); print $i}}' <<<"${viewport_line}")"
viewport_height="$(awk '{for (i = 1; i <= NF; i++) if ($i ~ /^height=/) {sub(/^height=/, "", $i); print $i}}' <<<"${viewport_line}")"
move_in_window "$((current_width - viewport_width + 45))" \
  "$((current_height - viewport_height + 10))"
xdotool click --repeat 2 --delay 50 1
xdotool key --clearmodifiers ctrl+shift+c
sleep 0.2
clipboard_text="$(clipboard_get 2>/dev/null || true)"
if [[ "${clipboard_text}" != *kitmuxcopytoken* ]]; then
  echo "Terminal selection did not round-trip through the GTK clipboard." >&2
  printf 'clipboard=%q\n' "${clipboard_text}" >&2
  cat "${log}" >&2
  exit 1
fi

# Enable SGR button-motion reporting in a foreground reader, then drive the
# production press/drag/release and wheel controller path. Shift is reserved
# for local selection/scrolling and is never forwarded as a terminal modifier.
move_in_window 450 320
xdotool click 1
xdotool type --clearmodifiers --delay 1 -- \
  "(i=0; while [ \$i -lt 100 ]; do printf 'pointer-flood-%03d\\n' \"\$i\"; i=\$((i + 1)); sleep 0.02; done; printf pointer-flood-ok > \"${pointer_flood_marker}\") & stty -echo; printf '\\033[?1002h\\033[?1006h'; timeout ${mouse_reader_seconds} cat -v; printf '\\033[?1002l\\033[?1006l'; stty sane; printf mouse-test-ok > \"${mouse_marker}\""
xdotool key --clearmodifiers Return
sleep 0.3
move_in_window 500 220
xdotool mousedown 1
move_in_window 560 240
xdotool mouseup 1
xdotool click 5
wait_for_log '^kitmux event=mouse_forwarded button=1 action=0 ' "mouse press reporting"
wait_for_log '^kitmux event=mouse_forwarded button=1 action=2 ' "mouse drag reporting"
wait_for_log '^kitmux event=mouse_forwarded button=1 action=1 ' "mouse release reporting"
wait_for_log '^kitmux event=scroll_raw dy=' "GTK wheel controller input"
wait_for_file "${pointer_flood_marker}" "PTY progress during pointer activity"
if [[ "${backend}" == "wayland" ]]; then
  wait_for_file "${mouse_marker}" "return from mouse-reporting foreground reader"
  # XTEST targets Weston's outer X11 window, so Alt-F4 would close the
  # compositor rather than issue a Wayland toplevel close. X11 already proves
  # the product confirmation path. End the already-exercised shell directly
  # so this run can verify the native Wayland child-exit lifecycle.
  kill -HUP "${child_pid}"
else
  # Closing a live foreground job needs confirmation.
  # Deterministically cancel the first request, then confirm the second after
  # the app rechecks the PTY foreground group.
xdotool key --clearmodifiers alt+F4
wait_for_log '^kitmux event=close_cancelled$' "cancelled foreground close"
if ! kill -0 "${app_pid}" 2>/dev/null; then
  echo "Kitmux exited after the cancelled foreground close." >&2
  exit 1
fi
xdotool key --clearmodifiers alt+F4
wait_for_log '^kitmux event=close_confirmed foreground_rechecked=true( |$)' \
  "confirmed foreground close"
fi
for _ in $(seq 1 200); do
  ! kill -0 "${app_pid}" 2>/dev/null && break
  sleep 0.1
done
if kill -0 "${app_pid}" 2>/dev/null; then
  echo "Kitmux did not exit after its shell exited." >&2
  if kill -0 "${child_pid}" 2>/dev/null; then
    echo "Shell child ${child_pid} is still alive." >&2
  else
    echo "Shell child ${child_pid} exited, but the window remained alive." >&2
  fi
  cat "${log}" >&2
  exit 1
fi
if ! wait "${app_pid}"; then
  echo "Kitmux exited unsuccessfully." >&2
  exit 1
fi
app_pid=""
if kill -0 "${child_pid}" 2>/dev/null; then
  echo "Kitmux left shell child ${child_pid} alive." >&2
  exit 1
fi
child_pid=""

if [[ ! -f "${state_path}" || ! -f "${state_path}.last-good" ]]; then
  echo "Kitmux did not write primary and last-good state snapshots." >&2
  exit 1
fi
if [[ "$(stat -c '%a' "${state_path}")" != 600 ]] \
    || [[ "$(stat -c '%a' "${state_path}.last-good")" != 600 ]]; then
  echo "Kitmux state files are not private." >&2
  exit 1
fi
python3 - "${settings_path}" "${state_path}" "${command_dir}" <<'PY'
import json
import pathlib
import sys

settings_path, state_path, expected_cwd = map(pathlib.Path, sys.argv[1:])
settings = json.loads(settings_path.read_text())
state = json.loads(state_path.read_text())
assert settings["gateSentinel"] == "kept"
assert state["version"] == 1
assert state["fontSize"] > 0
detail = next(iter(state["workspaces"][0]["tabGroups"][0]["terminalTabs"][0]["paneDetails"].values()))
assert detail["surfaces"]
assert all(pathlib.Path(surface["cwd"]) == expected_cwd for surface in detail["surfaces"])
assert all("resumeCommand" not in surface for surface in detail["surfaces"])
assert "resumeCommand" not in detail
PY

if grep -Fq "${linux_root}" "${log}" || grep -q 'LD_LIBRARY_PATH' "${log}" \
    || grep -q 'must-not-appear-in-diagnostics' "${log}"; then
  echo "Phase 4 diagnostics leaked a developer path, loader override, or secret canary." >&2
  exit 1
fi

echo "Phase 4 release-layout lifecycle and terminal-interaction gate: OK"
