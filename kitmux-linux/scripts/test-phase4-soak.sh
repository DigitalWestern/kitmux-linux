#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" || -z "${DISPLAY:-}" ]]; then
  echo "Run this gate on Linux with DISPLAY set." >&2
  exit 1
fi
for command in install realpath stat timeout xdotool; do
  command -v "${command}" >/dev/null || {
    echo "Missing required command: ${command}" >&2
    exit 1
  }
done

duration="${KITMUX_PHASE4_SOAK_SECONDS:-1800}"
if ! [[ "${duration}" =~ ^[1-9][0-9]*$ ]]; then
  echo "KITMUX_PHASE4_SOAK_SECONDS must be a positive integer." >&2
  exit 1
fi

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "${script_dir}/gate-common.sh"
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/kitmux-phase4-soak.XXXXXX")"
runtime="${temporary_root}/runtime"
command_dir="${temporary_root}/cwd"
heartbeat="${temporary_root}/heartbeat"
flood_pid_path="${temporary_root}/flood.pid"
log="${temporary_root}/kitmux.log"
app_pid=""
child_pid=""
flood_pid=""

cleanup() {
  if [[ -n "${flood_pid}" ]] && kill -0 "${flood_pid}" 2>/dev/null; then
    kill "${flood_pid}" 2>/dev/null || true
  fi
  if [[ -n "${child_pid}" ]] && kill -0 "${child_pid}" 2>/dev/null; then
    kill -HUP "${child_pid}" 2>/dev/null || true
  fi
  if [[ -n "${app_pid}" ]] && kill -0 "${app_pid}" 2>/dev/null; then
    kill "${app_pid}" 2>/dev/null || true
    wait "${app_pid}" 2>/dev/null || true
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
  echo "Kitmux never reported ${description}." >&2
  cat "${log}" >&2
  exit 1
}

monotonic_ms() {
  local seconds fraction rest
  IFS='. ' read -r seconds fraction rest < /proc/uptime
  fraction="${fraction}000"
  printf '%d\n' "$((10#${seconds} * 1000 + 10#${fraction:0:3}))"
}

KITMUX_BUILD_APP_RUNTIME=1 KITMUX_APP_TEST_HOOKS=ON \
  "${script_dir}/build-release-runtime.sh" "${runtime}"
install -d -m 700 "${command_dir}" "${temporary_root}/config" \
  "${temporary_root}/state" "${temporary_root}/data" "${temporary_root}/cache"

env -i DISPLAY="${DISPLAY}" HOME="${HOME}" LANG="${LANG:-C.UTF-8}" \
  PATH=/usr/bin:/bin GSK_RENDERER=gl GTK_IM_MODULE=gtk-im-context-simple \
  GTK_USE_PORTAL=0 GIO_USE_PORTAL=0 \
  KITMUX_PHASE4_CWD="${command_dir}" KITMUX_INTERACTION_DIAGNOSTICS=1 \
  XDG_CONFIG_HOME="${temporary_root}/config" \
  XDG_STATE_HOME="${temporary_root}/state" \
  XDG_DATA_HOME="${temporary_root}/data" \
  XDG_CACHE_HOME="${temporary_root}/cache" \
  "${runtime}/bin/kitmux" >"${log}" 2>&1 &
app_pid=$!
wait_for_log '^kitmux event=terminal_ready pid=[0-9]+.*backend=GdkX11Display$' \
  "X11 terminal readiness"
child_pid="$(sed -n 's/^kitmux event=terminal_ready pid=\([0-9][0-9]*\).*/\1/p' \
  "${log}" | head -n 1)"
kill -0 "${child_pid}" 2>/dev/null

window_id=""
for _ in $(seq 1 200); do
  window_id="$(xdotool search --onlyvisible --pid "${app_pid}" 2>/dev/null \
    | head -n 1 || true)"
  [[ -n "${window_id}" ]] && break
  sleep 0.1
done
[[ -n "${window_id}" ]] || {
  echo "Could not find the Kitmux soak window." >&2
  exit 1
}

xdotool windowactivate --sync "${window_id}"
xdotool mousemove --window "${window_id}" 400 260
xdotool click 1
flood_command="(i=0; while :; do i=\$((i + 1)); printf 'kitmux-soak-%08d\\n' \"\$i\"; done) & printf '%s\\n' \"\$!\" > \"${flood_pid_path}\""
xdotool type --clearmodifiers --delay 1 -- "${flood_command}"
xdotool key --clearmodifiers Return
for _ in $(seq 1 200); do
  [[ -s "${flood_pid_path}" ]] && break
  sleep 0.1
done
flood_pid="$(<"${flood_pid_path}")"
[[ "${flood_pid}" =~ ^[0-9]+$ ]] && kill -0 "${flood_pid}" 2>/dev/null

sizes=("760 480" "940 620" "800 520" "920 600" "780 500" "900 580")
start_ms="$(monotonic_ms)"
deadline_ms=$((start_ms + duration * 1000))
next_heartbeat_ms="${start_ms}"
next_progress_ms=$((start_ms + 60000))
iterations=0
heartbeats=0
max_heartbeat_ms=0

while (( $(monotonic_ms) < deadline_ms )); do
  kill -0 "${app_pid}" 2>/dev/null || {
    echo "Kitmux exited during the soak." >&2
    cat "${log}" >&2
    exit 1
  }
  kill -0 "${child_pid}" 2>/dev/null || {
    echo "The shell exited during the soak." >&2
    exit 1
  }
  kill -0 "${flood_pid}" 2>/dev/null || {
    echo "The PTY flood worker exited during the soak." >&2
    exit 1
  }

  read -r width height <<<"${sizes[$((iterations % ${#sizes[@]}))]}"
  timeout 5 xdotool windowsize --sync "${window_id}" "${width}" "${height}"
  timeout 5 xdotool mousemove --window "${window_id}" \
    $((260 + iterations % 240)) $((180 + iterations % 220))
  timeout 5 xdotool click 1
  if (( iterations % 2 == 0 )); then
    timeout 5 xdotool click 4
  else
    timeout 5 xdotool click 5
  fi
  if (( iterations % 30 == 0 )); then
    timeout 5 xdotool key --clearmodifiers ctrl+shift+equal
    timeout 5 xdotool key --clearmodifiers ctrl+minus
    timeout 5 xdotool key --clearmodifiers ctrl+0
  fi

  now_ms="$(monotonic_ms)"
  if (( now_ms >= next_heartbeat_ms )); then
    heartbeats=$((heartbeats + 1))
    heartbeat_started_ms="$(monotonic_ms)"
    timeout 5 xdotool type --clearmodifiers --delay 0 -- \
      "printf '%s' '${heartbeats}' > '${heartbeat}'"
    timeout 5 xdotool key --clearmodifiers Return
    for _ in $(seq 1 50); do
      [[ -f "${heartbeat}" && "$(<"${heartbeat}")" == "${heartbeats}" ]] && break
      sleep 0.1
    done
    if [[ ! -f "${heartbeat}" || "$(<"${heartbeat}")" != "${heartbeats}" ]]; then
      echo "Shell heartbeat ${heartbeats} missed its five-second bound." >&2
      exit 1
    fi
    heartbeat_ms=$(( $(monotonic_ms) - heartbeat_started_ms ))
    (( heartbeat_ms > max_heartbeat_ms )) && max_heartbeat_ms="${heartbeat_ms}"
    next_heartbeat_ms=$((now_ms + 10000))
  fi
  if (( now_ms >= next_progress_ms )); then
    echo "soak progress: $(((now_ms - start_ms) / 1000))/${duration}s, iterations=${iterations}, heartbeats=${heartbeats}, max-heartbeat=${max_heartbeat_ms}ms"
    next_progress_ms=$((now_ms + 60000))
  fi
  iterations=$((iterations + 1))
  sleep 1
done

elapsed=$(( ($(monotonic_ms) - start_ms) / 1000 ))
(( elapsed >= duration ))
kill "${flood_pid}"
for _ in $(seq 1 100); do
  ! kill -0 "${flood_pid}" 2>/dev/null && break
  sleep 0.1
done
if kill -0 "${flood_pid}" 2>/dev/null; then
  echo "PTY flood worker did not stop." >&2
  exit 1
fi
flood_pid=""

kill -HUP "${child_pid}"
for _ in $(seq 1 200); do
  ! kill -0 "${app_pid}" 2>/dev/null && break
  sleep 0.1
done
if kill -0 "${app_pid}" 2>/dev/null; then
  echo "Kitmux did not close after the soak shell exited." >&2
  exit 1
fi
wait "${app_pid}"
app_pid=""
if kill -0 "${child_pid}" 2>/dev/null; then
  echo "Kitmux left shell child ${child_pid} alive after the soak." >&2
  exit 1
fi
child_pid=""

viewport_events="$(grep -c '^kitmux event=viewport ' "${log}" || true)"
pointer_events="$(grep -c '^kitmux event=pointer_press ' "${log}" || true)"
scroll_events="$(grep -c '^kitmux event=scroll_raw ' "${log}" || true)"
font_events="$(grep -c '^kitmux event=font_size ' "${log}" || true)"
minimum_iterations=$((duration / 3))
(( iterations >= minimum_iterations ))
(( viewport_events >= minimum_iterations ))
(( pointer_events >= minimum_iterations ))
(( scroll_events >= minimum_iterations ))
(( heartbeats >= (duration + 19) / 20 ))
(( font_events >= duration / 120 ))
[[ "$(stat -c '%a' "${temporary_root}/state/kitmux/state.json")" == 600 ]]

if (( duration >= 1800 )); then
  echo "Phase 4 30-minute flood/resize/interaction soak: OK (${elapsed}s, ${iterations} iterations, ${heartbeats} heartbeats, ${max_heartbeat_ms}ms max heartbeat)"
else
  echo "Phase 4 soak smoke: OK (${elapsed}s, ${iterations} iterations, ${heartbeats} heartbeats, ${max_heartbeat_ms}ms max heartbeat)"
fi
