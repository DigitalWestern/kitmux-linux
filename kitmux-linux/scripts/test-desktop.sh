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
linux_root="$(cd -- "${source_root}/.." && pwd)"
kitty_root="${linux_root}/.source/kitty"
reference_root="${linux_root}/.source/reference"
build_dir="${source_root}/build-gtk"
host_binary="${build_dir}/kitmux_gtk_host"
proof_image="${source_root}/gtk-terminal-host-proof.png"
keyboard_proof_image="${source_root}/gtk-keyboard-focus-proof.png"
export DISPLAY=":${display_number}"

case "$(uname -m)" in
  aarch64|arm64) kitty_platform="linux-arm64" ;;
  x86_64|amd64) kitty_platform="linux-64" ;;
  *)
    echo "Unsupported Linux architecture: $(uname -m)" >&2
    exit 1
    ;;
esac
dependencies="${kitty_root}/dependencies/${kitty_platform}"
python_runtime="${build_dir}/python-runtime"

xdpyinfo >/dev/null
curl --fail --silent "http://127.0.0.1:${novnc_port}/vnc.html" >/dev/null

if [[ ! -x "${dependencies}/bin/python" ]] \
    || [[ ! -f "${kitty_root}/kitty/fast_data_types.so" ]]; then
  echo "Build the pinned Kitty development runtime before the desktop gate." >&2
  exit 1
fi

mapfile -t bundled_python_libraries < <(
  find "${dependencies}/lib" -mindepth 1 -maxdepth 1 -type f \
    -name 'libpython3.*.so.1.0' -print
)
if [[ "${#bundled_python_libraries[@]}" -ne 1 ]]; then
  echo "Expected one bundled libpython for the GTK host." >&2
  exit 1
fi
mkdir -p "${python_runtime}"
install -m 0755 "${bundled_python_libraries[0]}" "${python_runtime}/"
isolated_python_library="${python_runtime}/$(basename -- "${bundled_python_libraries[0]}")"

cmake -S "${source_root}" -B "${build_dir}" \
  -DCMAKE_BUILD_TYPE=RelWithDebInfo \
  -DKITMUX_BUILD_GTK_HOST=ON \
  -DPython3_ROOT_DIR="${dependencies}" \
  -DPython3_EXECUTABLE="${dependencies}/bin/python" \
  -DPython3_FIND_STRATEGY=LOCATION \
  -DPython3_FIND_UNVERSIONED_NAMES=FIRST \
  -DKITMUX_PYTHON_LIBRARY_OVERRIDE="${isolated_python_library}"
cmake --build "${build_dir}" --parallel \
  --target kitmux_gtk_host gtk_key_matrix pty_input_recorder x11_key_injector

gtk_version="$(pkg-config --modversion gtk4)"
renderer="$(glxinfo -B | awk -F: '/OpenGL renderer string/ {sub(/^[[:space:]]+/, "", $2); print $2}')"

if [[ -z "${renderer}" ]]; then
  echo "Could not identify an OpenGL renderer." >&2
  exit 1
fi

echo "GTK ${gtk_version}"
echo "OpenGL renderer: ${renderer}"
echo "X11 DISPLAY=${DISPLAY} and noVNC are responding"

proof_log="$(mktemp /tmp/kitmux-gtk-host.XXXXXX.log)"
error_log="$(mktemp /tmp/kitmux-gtk-error.XXXXXX.log)"
key_dir="$(mktemp -d /tmp/kitmux-gtk-keys.XXXXXX)"
cleanup() {
  if [[ -n "${host_pid:-}" ]] && kill -0 "${host_pid}" 2>/dev/null; then
    kill "${host_pid}" 2>/dev/null || true
    wait "${host_pid}" 2>/dev/null || true
  fi
  xset r on 2>/dev/null || true
  rm -rf -- "${proof_log}" "${error_log}" "${key_dir}"
}
trap cleanup EXIT

env \
  GTK_THEME=Adwaita \
  GSK_RENDERER=gl \
  PYTHONHOME="${dependencies}" \
  KITTY_SRC="${kitty_root}" \
  LIBKITTY_PY="${reference_root}/libkitty/py" \
  LIBKITTY_TEST_CONFIG="${reference_root}/libkitty/tests/fixtures/kitty.conf" \
  KITMUX_GTK_AUTO_CLOSE_MS=5000 \
  "${host_binary}" >"${proof_log}" 2>&1 &
host_pid=$!

window_id="$(
  timeout 12s xdotool search --sync --onlyvisible --pid "${host_pid}" \
    | head -n 1
)"
xdotool windowactivate --sync "${window_id}"
xdotool windowsize --sync "${window_id}" 760 500
sleep 1
xdotool windowsize --sync "${window_id}" 980 640
sleep 1
scrot --overwrite --focused "${proof_image}"
wait "${host_pid}"
host_pid=""

cat "${proof_log}"
grep -q '^GTK terminal ready:' "${proof_log}"
grep -q '^GTK terminal PTY output:' "${proof_log}"
grep -q '^GTK terminal GL state restoration: OK' "${proof_log}"
if [[ "$(grep -c '^GTK terminal viewport:' "${proof_log}")" -lt 2 ]]; then
  echo "GTK host did not report both initial and resized viewports." >&2
  exit 1
fi
child_pid="$(sed -n 's/^GTK terminal frame: .* pid=\([0-9][0-9]*\)$/\1/p' "${proof_log}" | head -n 1)"
if [[ -z "${child_pid}" ]]; then
  echo "GTK host did not report its terminal child PID." >&2
  exit 1
fi
if kill -0 "${child_pid}" 2>/dev/null; then
  echo "GTK host left terminal child ${child_pid} alive after close." >&2
  exit 1
fi

echo "Real GTK/libkitty render, resize, PTY, and clean-close proof: OK"

# ---------------------------------------------------------------------------
# Slice 2.2A keyboard harness
# ---------------------------------------------------------------------------

libkitty_environment=(
  PYTHONHOME="${dependencies}"
  KITTY_SRC="${kitty_root}"
  LIBKITTY_PY="${reference_root}/libkitty/py"
  LIBKITTY_TEST_CONFIG="${reference_root}/libkitty/tests/fixtures/kitty.conf"
)
recorder="${build_dir}/pty_input_recorder"

# Display-free half: every documented key, in three live terminal states,
# against fixed expected bytes, delivered to a real PTY child.
env "${libkitty_environment[@]}" \
  KITMUX_RECORDER_BIN="${recorder}" \
  KITMUX_RECORDER_LOG_DIR="${key_dir}" \
  "${build_dir}/gtk_key_matrix"

# X11 half: the same translation driven by real GDK events through the real
# focused widget. Auto-repeat stays off except for the deliberate repeat
# phase, so no key produces events the script did not ask for.
xset r off

inject="${build_dir}/x11_key_injector"

wait_for_line() {  # log, regex, description
  local log="$1" pattern="$2" description="$3"
  for _ in $(seq 1 200); do
    if grep -qE "${pattern}" "${log}" 2>/dev/null; then return 0; fi
    sleep 0.1
  done
  echo "GTK host never reported ${description}; log follows:" >&2
  cat "${log}" >&2
  exit 1
}

wait_for_count() {  # log, regex, count, description
  local log="$1" pattern="$2" want="$3" description="$4"
  for _ in $(seq 1 200); do
    if [[ "$(grep -cE "${pattern}" "${log}" 2>/dev/null || true)" -ge "${want}" ]]
    then
      return 0
    fi
    sleep 0.1
  done
  echo "GTK host never reported ${want}x ${description}; log follows:" >&2
  cat "${log}" >&2
  exit 1
}

child_hex() { sed -n 's/^bytes //p' "$1" 2>/dev/null | tr -d '\n'; }

click_widget() {  # host log, bounds label
  local geometry
  geometry="$(sed -n "s/^GTK bounds $2: x=\([0-9-]*\) y=\([0-9-]*\) w=\([0-9-]*\) h=\([0-9-]*\)$/\1 \2 \3 \4/p" "$1" | head -n 1)"
  if [[ -z "${geometry}" ]]; then
    echo "GTK host did not report bounds for $2." >&2
    exit 1
  fi
  read -r bx by bw bh <<<"${geometry}"
  xdotool mousemove --window "${window_id}" \
    "$((bx + bw / 2))" "$((by + bh / 2))" click 1
}

launch_key_host() {  # label, recorder init sequence, recorder quit hex
  key_label="$1"
  child_log="${key_dir}/gui-${key_label}.log"
  host_log="${key_dir}/gui-${key_label}-host.log"
  rm -f -- "${child_log}" "${host_log}"
  env "${libkitty_environment[@]}" \
    GTK_THEME=Adwaita \
    GSK_RENDERER=gl \
    KITMUX_GTK_CHILD="${recorder}" \
    KITMUX_RECORDER_LOG="${child_log}" \
    KITMUX_RECORDER_INIT="$2" \
    KITMUX_RECORDER_QUIT="$3" \
    KITMUX_GTK_CLOSE_ON_CHILD_EXIT=1 \
    KITMUX_GTK_AUTO_CLOSE_MS=90000 \
    "${host_binary}" >"${host_log}" 2>&1 &
  host_pid=$!
  window_id="$(
    timeout 20s xdotool search --sync --onlyvisible --pid "${host_pid}" \
      | head -n 1
  )"
  xdotool windowactivate --sync "${window_id}"
  # The fixture's startup write is the only child output at this point, so
  # this line proves any requested terminal mode reached kitty's Screen.
  wait_for_line "${host_log}" '^GTK terminal PTY output:' "child startup output"
  wait_for_line "${host_log}" '^GTK bounds adjacent-control:' "widget bounds"
  wait_for_line "${host_log}" '^GTK focus: terminal$' "initial terminal focus"
}

finish_key_host() {  # expected child byte stream, as an extended regex
  wait "${host_pid}"
  host_pid=""
  local recorded
  recorded="$(child_hex "${child_log}")"
  if [[ ! "${recorded}" =~ $1 ]]; then
    echo "GTK ${key_label} run child bytes did not match expectations." >&2
    echo "  recorded: ${recorded}" >&2
    echo "  expected: $1" >&2
    cat "${host_log}" >&2
    exit 1
  fi
  child_pid="$(sed -n 's/^GTK terminal frame: .* pid=\([0-9][0-9]*\)$/\1/p' "${host_log}" | head -n 1)"
  if [[ -z "${child_pid}" ]] || kill -0 "${child_pid}" 2>/dev/null; then
    echo "GTK ${key_label} run left terminal child '${child_pid}' unreaped." >&2
    exit 1
  fi
}

hold_key_for_repeats() {  # keysym
  # X auto-repeat is the only way to produce a real GDK repeat event, and the
  # repeat count is timing-dependent. The assertions therefore fix the bytes
  # of every repeat and require at least one, not an exact multiplicity.
  xset r on
  xset r rate 200 20
  "${inject}" hold "$1" 600
  xset r off
}

# --- Legacy terminal state: default DECCKM off, no keyboard protocol --------
launch_key_host legacy '' '1b5b32347e'
"${inject}" \
  tap a \
  down Shift_L tap b up Shift_L \
  down Control_L tap c up Control_L \
  down Alt_L tap d up Alt_L \
  tap Return tap Tab tap BackSpace tap Escape \
  tap Up tap Down tap Left tap Right \
  tap F1
hold_key_for_repeats a
wait_for_count "${host_log}" '^GTK key repeat: key=0x61 .*bytes=61$' 1 \
  "an auto-repeat of 'a' encoded as 0x61"

# Focus transfer: the ordinary GTK control must take the keys, and the
# terminal must receive nothing while it does not have focus.
before_focus_transfer="$(child_hex "${child_log}")"
click_widget "${host_log}" adjacent-control
wait_for_line "${host_log}" '^GTK focus: adjacent-control$' "adjacent control focus"
xdotool type --delay 60 'xy'
wait_for_line "${host_log}" '^GTK adjacent control text: xy$' "adjacent control text"
if [[ "$(child_hex "${child_log}")" != "${before_focus_transfer}" ]]; then
  echo "Terminal received bytes while the adjacent GTK control had focus." >&2
  exit 1
fi
click_widget "${host_log}" terminal
wait_for_count "${host_log}" '^GTK focus: terminal$' 2 "terminal focus return"
"${inject}" tap z
wait_for_line "${host_log}" '^GTK key press: key=0x7A .*bytes=7a$' \
  "the terminal receiving a key after focus returned"
# llvmpipe repaints the GL area a frame or two behind the log, so let the
# window settle before capturing the visible proof.
sleep 2
scrot --overwrite --focused "${keyboard_proof_image}"
"${inject}" tap F12
finish_key_host '^6142031b640d097f1b1b5b411b5b421b5b441b5b431b4f50(61)+7a1b5b32347e$'
echo "Legacy-state GTK key routing and focus-transfer proof: OK"

# --- kitty keyboard protocol: press, repeat, and release all carry bytes ----
launch_key_host enhanced '\e[>15u' '1b5b32343b313a337e'
"${inject}" tap a tap Return tap Up
hold_key_for_repeats a
wait_for_count "${host_log}" '^GTK key repeat: key=0x61 .*bytes=1b5b39373b313a3275$' 1 \
  "an auto-repeat of 'a' encoded as a CSI u repeat event"
"${inject}" tap F12
finish_key_host '^1b5b3937751b5b39373b313a33751b5b3133751b5b31333b313a33751b5b411b5b313b313a33411b5b393775(1b5b39373b313a3275)+1b5b39373b313a33751b5b32347e1b5b32343b313a337e$'
echo "kitty-keyboard-protocol press/repeat/release proof: OK"

xset r on

env -u PYTHONHOME -u KITTY_SRC -u LIBKITTY_PY -u LIBKITTY_TEST_CONFIG \
  GTK_THEME=Adwaita \
  GSK_RENDERER=gl \
  KITMUX_GTK_AUTO_CLOSE_MS=1000 \
  "${host_binary}" >"${error_log}" 2>&1
grep -q '^Missing runtime paths\.' "${error_log}"

echo "Visible missing-runtime diagnostic proof: OK"
echo "GTK terminal host binary: ${host_binary}"
echo "Visible proof: ${proof_image}"
echo "Visible keyboard proof: ${keyboard_proof_image}"
