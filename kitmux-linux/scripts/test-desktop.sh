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
cmake --build "${build_dir}" --parallel --target kitmux_gtk_host

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
cleanup() {
  if [[ -n "${host_pid:-}" ]] && kill -0 "${host_pid}" 2>/dev/null; then
    kill "${host_pid}" 2>/dev/null || true
    wait "${host_pid}" 2>/dev/null || true
  fi
  rm -f -- "${proof_log}" "${error_log}"
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

env -u PYTHONHOME -u KITTY_SRC -u LIBKITTY_PY -u LIBKITTY_TEST_CONFIG \
  GTK_THEME=Adwaita \
  GSK_RENDERER=gl \
  KITMUX_GTK_AUTO_CLOSE_MS=1000 \
  "${host_binary}" >"${error_log}" 2>&1
grep -q '^Missing runtime paths\.' "${error_log}"

echo "Real GTK/libkitty render, resize, PTY, and clean-close proof: OK"
echo "Visible missing-runtime diagnostic proof: OK"
echo "GTK terminal host binary: ${host_binary}"
echo "Visible proof: ${proof_image}"
