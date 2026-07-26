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
preedit_proof_image="${source_root}/gtk-preedit-proof.png"
wayland_proof_image="${source_root}/gtk-wayland-proof.png"
webkit_x11_proof_image="${source_root}/gtk-webkit-x11-proof.png"
webkit_wayland_proof_image="${source_root}/gtk-webkit-wayland-proof.png"
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
webkit_version="$(pkg-config --modversion webkitgtk-6.0)"
renderer="$(glxinfo -B | awk -F: '/OpenGL renderer string/ {sub(/^[[:space:]]+/, "", $2); print $2}')"

if [[ -z "${renderer}" ]]; then
  echo "Could not identify an OpenGL renderer." >&2
  exit 1
fi

echo "GTK ${gtk_version}"
echo "WebKitGTK ${webkit_version}"
echo "OpenGL renderer: ${renderer}"
echo "X11 DISPLAY=${DISPLAY} and noVNC are responding"

if ! readelf -d "${host_binary}" \
    | grep -q 'Shared library: \[libwebkitgtk-6\.0\.so\.4\]'; then
  echo "GTK host does not directly declare the WebKitGTK 6.0 runtime." >&2
  exit 1
fi
host_closure="$(ldd "${host_binary}")"
if grep -q 'not found' <<<"${host_closure}"; then
  echo "GTK/WebKit host has an unresolved runtime library:" >&2
  grep 'not found' <<<"${host_closure}" >&2
  exit 1
fi
if ! grep -qE "libpython3\..* => ${python_runtime}/libpython3\..*\.so\.1\.0" \
    <<<"${host_closure}"; then
  echo "GTK host did not resolve libpython from its isolated runtime." >&2
  exit 1
fi
for system_library in libwebkitgtk-6.0.so.4 libjavascriptcoregtk-6.0.so.1 libgtk-4.so.1; do
  if ! grep -qE "${system_library//./\\.} => /usr/lib/[^/]+/${system_library//./\\.}" \
      <<<"${host_closure}"; then
    echo "GTK host did not resolve ${system_library} from the distro runtime." >&2
    exit 1
  fi
done
if grep -Fq "${dependencies}/lib" <<<"${host_closure}"; then
  echo "GTK/WebKit host leaked the Kitty development dependency directory into its loader closure." >&2
  exit 1
fi
host_closure_count="$(grep -c ' => /' <<<"${host_closure}")"
echo "WebKitGTK host native closure: ${host_closure_count} resolved libraries"

proof_log="$(mktemp /tmp/kitmux-gtk-host.XXXXXX.log)"
error_log="$(mktemp /tmp/kitmux-gtk-error.XXXXXX.log)"
key_dir="$(mktemp -d /tmp/kitmux-gtk-keys.XXXXXX)"
wayland_runtime_dir="$(mktemp -d /tmp/kitmux-wayland-runtime.XXXXXX)"
wayland_host_log="$(mktemp /tmp/kitmux-gtk-wayland-host.XXXXXX.log)"
wayland_child_log="$(mktemp /tmp/kitmux-gtk-wayland-child.XXXXXX.log)"
webkit_wayland_host_log="$(mktemp /tmp/kitmux-gtk-webkit-wayland-host.XXXXXX.log)"
webkit_wayland_child_log="$(mktemp /tmp/kitmux-gtk-webkit-wayland-child.XXXXXX.log)"
weston_log="$(mktemp /tmp/kitmux-weston.XXXXXX.log)"
cleanup() {
  if [[ -n "${host_pid:-}" ]] && kill -0 "${host_pid}" 2>/dev/null; then
    kill "${host_pid}" 2>/dev/null || true
    wait "${host_pid}" 2>/dev/null || true
  fi
  if [[ -n "${weston_pid:-}" ]] && kill -0 "${weston_pid}" 2>/dev/null; then
    kill "${weston_pid}" 2>/dev/null || true
    wait "${weston_pid}" 2>/dev/null || true
  fi
  xset r on 2>/dev/null || true
  setxkbmap -layout us 2>/dev/null || true
  rm -rf -- "${proof_log}" "${error_log}" "${key_dir}" \
    "${wayland_runtime_dir}" "${wayland_host_log}" "${wayland_child_log}" \
    "${webkit_wayland_host_log}" "${webkit_wayland_child_log}" \
    "${weston_log}"
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

key_host_env=()

launch_key_host() {  # label, recorder init sequence, recorder quit hex
  key_label="$1"
  child_log="${key_dir}/gui-${key_label}.log"
  host_log="${key_dir}/gui-${key_label}-host.log"
  rm -f -- "${child_log}" "${host_log}"
  env "${libkitty_environment[@]}" "${key_host_env[@]+"${key_host_env[@]}"}" \
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

# Pin GTK's own input-method context for the Slice 2.2A runs: a session with
# ibus-daemon already running would otherwise route ordinary typing through
# IBus and change which path encodes it.
key_host_env=(GTK_IM_MODULE=gtk-im-context-simple)

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

# ---------------------------------------------------------------------------
# Slice 2.2B Compose, layouts, and input methods
# ---------------------------------------------------------------------------

# GTK's own input-method context: Compose sequences and dead keys produce a
# preedit and commit composed text that must bypass the key encoder, while an
# AltGr level or a non-US layout is ordinary typing whose text only the input
# method knows.
setxkbmap -layout us -option compose:menu
launch_key_host compose '' '1b5b32347e'

"${inject}" tap Multi_key tap a tap e
wait_for_line "${host_log}" '^GTK preedit: "a" cursor=1 bytes=61$' \
  "a visible Compose preedit"
wait_for_line "${host_log}" \
  '^GTK input method commit: "æ" bytes=c3a6 route=direct-write$' \
  "the Compose result committed without the key encoder"
# The physical key whose press the input method swallowed must not leave a
# lone release behind it.
wait_for_line "${host_log}" '^GTK key release withheld: keyval=0x61 ' \
  "the withheld release of a swallowed press"

setxkbmap -layout us -variant intl -option compose:menu
sleep 1
"${inject}" tap dead_acute tap e
wait_for_line "${host_log}" '^GTK preedit: "'"'"'" cursor=1 bytes=27$' \
  "a visible dead-key preedit"
wait_for_line "${host_log}" \
  '^GTK input method commit: "é" bytes=c3a9 route=direct-write$' \
  "the dead-key result committed without the key encoder"

setxkbmap -layout de -option compose:menu
sleep 1
"${inject}" tap udiaeresis
wait_for_line "${host_log}" \
  '^GTK key press: key=0xFC .*text="ü" bytes=c3bc$' \
  "a non-US layout key encoded with its committed text"
"${inject}" down ISO_Level3_Shift tap q up ISO_Level3_Shift
# AltGr is a level shifter, not a kitty modifier: the key identity stays the
# unmodified symbol of the same hardware key, and the level's text rides along.
wait_for_line "${host_log}" \
  '^GTK key press: key=0x71 shifted=0x0 mods=0x0 text="@" bytes=40$' \
  "an AltGr level encoded as its base key plus committed text"

setxkbmap -layout us
sleep 1
"${inject}" tap F12
finish_key_host '^c3a6c3a9c3bc401b5b32347e$'
echo "Compose, dead-key, AltGr, and non-US layout proof: OK"

# A real IBus engine. m17n:t:latn-post holds a letter in the preedit until a
# following diacritic resolves it, so both the preedit and the commit are
# deterministic.
ibus_engines="['xkb:us::eng', 'm17n:t:latn-post']"
if ! gsettings set org.freedesktop.ibus.general preload-engines "${ibus_engines}"
then
  echo "Could not preload the IBus engines this gate needs." >&2
  exit 1
fi
if ! ibus-daemon --xim --daemonize --replace >/dev/null 2>&1; then
  echo "Could not start ibus-daemon." >&2
  exit 1
fi

use_ibus_engine() {  # engine name
  for _ in $(seq 1 60); do
    if [[ "$(ibus engine 2>/dev/null)" == "$1" ]]; then return 0; fi
    ibus engine "$1" >/dev/null 2>&1 || true
    sleep 0.5
  done
  echo "IBus never activated the '$1' engine; available engines:" >&2
  ibus list-engine >&2 2>&1 || true
  exit 1
}

use_ibus_engine xkb:us::eng
key_host_env=(GTK_IM_MODULE=ibus)
launch_key_host ibus '' '1b5b32347e'

# A pass-through engine must leave the key encoder in charge.
use_ibus_engine xkb:us::eng
sleep 1
"${inject}" tap a
wait_for_line "${host_log}" \
  '^GTK key press: key=0x61 shifted=0x0 mods=0x0 text="a" bytes=61$' \
  "a pass-through IBus engine leaving encoding to the key path"

use_ibus_engine m17n:t:latn-post
sleep 2
"${inject}" tap a
wait_for_line "${host_log}" '^GTK preedit: "a" cursor=1 bytes=61$' \
  "a real IBus preedit"
sleep 1
scrot --overwrite --focused "${preedit_proof_image}"
"${inject}" tap apostrophe
wait_for_line "${host_log}" '^GTK preedit: "á" cursor=1 bytes=c3a1$' \
  "the IBus preedit updating in place"
# latn-post holds the composed letter until something ends the composition;
# Return commits it and is then encoded as an ordinary key in its own right.
"${inject}" tap Return
wait_for_line "${host_log}" \
  '^GTK input method commit: "á" bytes=c3a1 route=direct-write$' \
  "the IBus commit bypassing the key encoder"
wait_for_line "${host_log}" '^GTK preedit end$' "the IBus preedit ending"
wait_for_line "${host_log}" '^GTK key press: key=0xE001 .*bytes=0d$' \
  "the composition-ending key encoded once, after the commit"

# A non-BMP scalar. Whether IBus answers inside the key filter or commits
# asynchronously afterwards is the engine's choice, so assert the bytes that
# reach the child rather than which of the two routes carried them.
xdotool type --delay 150 '🚀'
wait_for_line "${host_log}" \
  '^GTK input method commit: "🚀" bytes=f09f9a80 route=' \
  "an emoji commit reaching the child as UTF-8"

use_ibus_engine xkb:us::eng
"${inject}" tap F12
finish_key_host '^61c3a10df09f9a801b5b32347e$'
echo "IBus preedit, commit, pass-through, and emoji proof: OK"
key_host_env=()

# ---------------------------------------------------------------------------
# Slice 2.2D WebKitGTK conflict probe: X11
# ---------------------------------------------------------------------------

# This is deliberately a coexistence probe rather than browser product work:
# the host maps one WebKitWebView containing static in-memory HTML, transfers
# focus through it, and proves that Kitty resumes receiving the exact bytes.
key_host_env=(GTK_IM_MODULE=gtk-im-context-simple KITMUX_GTK_WEBKIT_PROBE=1)
launch_key_host webkit-x11 '' '1b5b32347e'
wait_for_line "${host_log}" '^GTK display backend: GdkX11Display$' \
  "the WebKit probe using X11"
wait_for_line "${host_log}" '^GTK terminal GL state restoration: OK$' \
  "GL state restoration beside WebKit under X11"
wait_for_line "${host_log}" '^GTK WebKit probe loaded: version=' \
  "the WebKit in-memory fixture loading under X11"
wait_for_line "${host_log}" \
  '^GTK WebKit probe document: Kitmux WebKit probe$' \
  "the exact WebKit in-memory document under X11"
wait_for_line "${host_log}" '^GTK bounds webkit-probe:' \
  "WebKit probe bounds under X11"
"${inject}" tap a
wait_for_line "${host_log}" '^GTK key press: key=0x61 .*bytes=61$' \
  "terminal input before the X11 WebKit focus transfer"
before_webkit_focus="$(child_hex "${child_log}")"
click_widget "${host_log}" webkit-probe
wait_for_line "${host_log}" '^GTK focus: webkit-probe$' \
  "WebKit focus under X11"
"${inject}" tap x
sleep 1
if [[ "$(child_hex "${child_log}")" != "${before_webkit_focus}" ]]; then
  echo "Terminal received bytes while WebKit held focus under X11." >&2
  exit 1
fi
sleep 1
scrot --overwrite --focused "${webkit_x11_proof_image}"
click_widget "${host_log}" terminal
wait_for_count "${host_log}" '^GTK focus: terminal$' 2 \
  "terminal focus before and after WebKit under X11"
"${inject}" tap z
wait_for_line "${host_log}" '^GTK key press: key=0x7A .*bytes=7a$' \
  "terminal input after the X11 WebKit focus transfer"
"${inject}" tap F12
finish_key_host '^617a1b5b32347e$'
if grep -qE '^GTK WebKit (probe load failed|process terminated):' "${host_log}"; then
  echo "WebKit failed or terminated during the X11 coexistence probe." >&2
  cat "${host_log}" >&2
  exit 1
fi
echo "WebKitGTK render, GL coexistence, and focus-return proof under X11: OK"
key_host_env=()

xset r on

# ---------------------------------------------------------------------------
# Slice 2.2C native Wayland client path
# ---------------------------------------------------------------------------

# This remains disposable spike harness code under ADR 0007. Weston runs
# nested in the dedicated X11 development desktop so the result stays visible,
# but the GTK host is a native Wayland client and Weston is started without
# Xwayland. XTEST drives only Weston's outer X11 window; Weston translates
# those events into wl_keyboard events for GTK. The injection evidence is
# therefore intentionally display-bound and is not physical-libinput proof.
wayland_socket="${KITMUX_WAYLAND_DISPLAY:-wayland-kitmux-test}"
if [[ -z "${wayland_socket}" || "${wayland_socket}" == */* ]]; then
  echo "KITMUX_WAYLAND_DISPLAY must be a non-empty Wayland socket name." >&2
  exit 1
fi
chmod 700 "${wayland_runtime_dir}"
env \
  DISPLAY="${DISPLAY}" \
  XDG_RUNTIME_DIR="${wayland_runtime_dir}" \
  weston \
    --backend=x11 \
    --renderer=gl \
    --width=1024 \
    --height=700 \
    --socket="${wayland_socket}" \
    --shell=kiosk \
    --no-config \
    --idle-time=0 \
    --log="${weston_log}" \
    >/dev/null 2>&1 &
weston_pid=$!

wayland_window_id=""
for _ in $(seq 1 120); do
  if ! kill -0 "${weston_pid}" 2>/dev/null; then
    echo "Weston exited before its Wayland socket became ready." >&2
    cat "${weston_log}" >&2
    exit 1
  fi
  if [[ -S "${wayland_runtime_dir}/${wayland_socket}" ]]; then
    wayland_window_id="$(
      sed -n 's/.*window id \([0-9][0-9]*\)$/\1/p' "${weston_log}" \
        | tail -n 1
    )"
    if [[ -n "${wayland_window_id}" ]]; then break; fi
  fi
  sleep 0.1
done
if [[ -z "${wayland_window_id}" ]]; then
  echo "Weston did not report a ready nested output." >&2
  cat "${weston_log}" >&2
  exit 1
fi
grep -q 'Using GL renderer' "${weston_log}"

use_ibus_engine xkb:us::eng
ibus_address="$(ibus address)"
if [[ -z "${ibus_address}" ]]; then
  echo "IBus did not report an address for the Wayland host." >&2
  exit 1
fi

key_label="wayland"
child_log="${wayland_child_log}"
host_log="${wayland_host_log}"
rm -f -- "${child_log}" "${host_log}"
env "${libkitty_environment[@]}" \
  DISPLAY="${DISPLAY}" \
  XDG_RUNTIME_DIR="${wayland_runtime_dir}" \
  WAYLAND_DISPLAY="${wayland_socket}" \
  GDK_BACKEND=wayland \
  GTK_IM_MODULE=ibus \
  IBUS_ADDRESS="${ibus_address}" \
  NO_AT_BRIDGE=1 \
  GTK_THEME=Adwaita \
  GSK_RENDERER=gl \
  KITMUX_GTK_CHILD="${recorder}" \
  KITMUX_RECORDER_LOG="${child_log}" \
  KITMUX_RECORDER_QUIT=1b5b32347e \
  KITMUX_GTK_CLOSE_ON_CHILD_EXIT=1 \
  KITMUX_GTK_AUTO_CLOSE_MS=90000 \
  "${host_binary}" >"${host_log}" 2>&1 &
host_pid=$!

wait_for_line "${host_log}" '^GTK display backend: GdkWaylandDisplay$' \
  "a native Wayland GDK display"
wait_for_line "${host_log}" '^GTK terminal PTY output:' "Wayland child startup"
wait_for_line "${host_log}" '^GTK terminal GL state restoration: OK$' \
  "Wayland GL state restoration"
wait_for_line "${host_log}" '^GTK focus: terminal$' \
  "initial Wayland terminal focus"

xdotool windowactivate --sync "${wayland_window_id}"
xdotool windowsize --sync "${wayland_window_id}" 760 500
wait_for_line "${host_log}" '^GTK terminal viewport: 760x450 scale=1$' \
  "the first Wayland framebuffer resize"
xdotool windowsize --sync "${wayland_window_id}" 980 640
wait_for_line "${host_log}" '^GTK terminal viewport: 980x590 scale=1$' \
  "the second Wayland framebuffer resize"

# One ordinary press/release plus a real compositor-generated repeat series.
xset r off
"${inject}" tap a
wait_for_line "${host_log}" \
  '^GTK key press: key=0x61 .*text="a" bytes=61$' \
  "a Wayland key press"
wait_for_line "${host_log}" \
  '^GTK key release: key=0x61 .*bytes=$' \
  "a Wayland key release"
hold_key_for_repeats a
wait_for_count "${host_log}" '^GTK key repeat: key=0x61 .*bytes=61$' 1 \
  "a Wayland auto-repeat encoded as 0x61"

# Reuse the deterministic m17n engine from Slice 2.2B. The explicit IBus
# address is necessary because the nested compositor has its own runtime dir.
use_ibus_engine m17n:t:latn-post
sleep 1
"${inject}" tap a
wait_for_line "${host_log}" '^GTK preedit: "a" cursor=1 bytes=61$' \
  "a real IBus preedit under Wayland"
sleep 1
scrot --overwrite --focused "${wayland_proof_image}"
"${inject}" tap apostrophe
wait_for_line "${host_log}" '^GTK preedit: "á" cursor=1 bytes=c3a1$' \
  "the Wayland IBus preedit updating in place"
"${inject}" tap Return
wait_for_line "${host_log}" \
  '^GTK input method commit: "á" bytes=c3a1 route=direct-write$' \
  "the Wayland IBus commit bypassing the key encoder"
wait_for_line "${host_log}" '^GTK preedit end$' \
  "the Wayland IBus preedit ending"
wait_for_line "${host_log}" '^GTK key press: key=0xE001 .*bytes=0d$' \
  "the Wayland composition-ending key encoded once"

use_ibus_engine xkb:us::eng
"${inject}" tap F12
finish_key_host '^61(61)+c3a10d1b5b32347e$'
xset r on

if [[ "$(grep -c '^GTK terminal viewport:' "${host_log}")" -lt 3 ]]; then
  echo "Wayland host did not report its initial and two resized viewports." >&2
  exit 1
fi
echo "Native Wayland render, resize, GL, keyboard, IBus, and clean-close proof: OK"
echo "Wayland injection boundary: XTEST -> Weston X11 backend -> wl_keyboard"

# ---------------------------------------------------------------------------
# Slice 2.2D WebKitGTK conflict probe: native Wayland
# ---------------------------------------------------------------------------

key_label="webkit-wayland"
child_log="${webkit_wayland_child_log}"
host_log="${webkit_wayland_host_log}"
rm -f -- "${child_log}" "${host_log}"
env "${libkitty_environment[@]}" \
  DISPLAY="${DISPLAY}" \
  XDG_RUNTIME_DIR="${wayland_runtime_dir}" \
  WAYLAND_DISPLAY="${wayland_socket}" \
  GDK_BACKEND=wayland \
  GTK_IM_MODULE=gtk-im-context-simple \
  NO_AT_BRIDGE=1 \
  GTK_THEME=Adwaita \
  GSK_RENDERER=gl \
  KITMUX_GTK_WEBKIT_PROBE=1 \
  KITMUX_GTK_CHILD="${recorder}" \
  KITMUX_RECORDER_LOG="${child_log}" \
  KITMUX_RECORDER_QUIT=1b5b32347e \
  KITMUX_GTK_CLOSE_ON_CHILD_EXIT=1 \
  KITMUX_GTK_AUTO_CLOSE_MS=90000 \
  "${host_binary}" >"${host_log}" 2>&1 &
host_pid=$!
window_id="${wayland_window_id}"

wait_for_line "${host_log}" '^GTK display backend: GdkWaylandDisplay$' \
  "the WebKit probe using native Wayland"
wait_for_line "${host_log}" '^GTK terminal PTY output:' \
  "the Wayland WebKit probe child startup"
wait_for_line "${host_log}" '^GTK terminal GL state restoration: OK$' \
  "GL state restoration beside WebKit under Wayland"
wait_for_line "${host_log}" '^GTK WebKit probe loaded: version=' \
  "the WebKit in-memory fixture loading under Wayland"
wait_for_line "${host_log}" \
  '^GTK WebKit probe document: Kitmux WebKit probe$' \
  "the exact WebKit in-memory document under Wayland"
wait_for_line "${host_log}" '^GTK bounds webkit-probe:' \
  "WebKit probe bounds under Wayland"
wait_for_line "${host_log}" '^GTK focus: terminal$' \
  "initial terminal focus beside WebKit under Wayland"

xset r off
xdotool windowactivate --sync "${wayland_window_id}"
"${inject}" tap a
wait_for_line "${host_log}" '^GTK key press: key=0x61 .*bytes=61$' \
  "terminal input before the Wayland WebKit focus transfer"
before_webkit_focus="$(child_hex "${child_log}")"
click_widget "${host_log}" webkit-probe
wait_for_line "${host_log}" '^GTK focus: webkit-probe$' \
  "WebKit focus under Wayland"
"${inject}" tap x
sleep 1
if [[ "$(child_hex "${child_log}")" != "${before_webkit_focus}" ]]; then
  echo "Terminal received bytes while WebKit held focus under Wayland." >&2
  exit 1
fi
sleep 1
scrot --overwrite --focused "${webkit_wayland_proof_image}"
click_widget "${host_log}" terminal
wait_for_count "${host_log}" '^GTK focus: terminal$' 2 \
  "terminal focus before and after WebKit under Wayland"
"${inject}" tap z
wait_for_line "${host_log}" '^GTK key press: key=0x7A .*bytes=7a$' \
  "terminal input after the Wayland WebKit focus transfer"
"${inject}" tap F12
finish_key_host '^617a1b5b32347e$'
xset r on
if grep -qE '^GTK WebKit (probe load failed|process terminated):' "${host_log}"; then
  echo "WebKit failed or terminated during the Wayland coexistence probe." >&2
  cat "${host_log}" >&2
  exit 1
fi
echo "WebKitGTK render, GL coexistence, and focus-return proof under native Wayland: OK"

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
echo "Visible preedit proof: ${preedit_proof_image}"
echo "Visible Wayland proof: ${wayland_proof_image}"
echo "Visible WebKitGTK X11 proof: ${webkit_x11_proof_image}"
echo "Visible WebKitGTK Wayland proof: ${webkit_wayland_proof_image}"
