#!/usr/bin/env bash
set -euo pipefail

runtime="${1:?usage: test-release-runtime.sh /path/to/runtime}"
runtime="$(realpath "${runtime}")"

required=(
  "${runtime}/bin/linux_session_stress"
  "${runtime}/etc/kitty.conf"
  "${runtime}/kitty/fast_data_types.so"
  "${runtime}/lib/libkitty.so"
  "${runtime}/lib/libpython3.14.so.1.0"
  "${runtime}/lib/python3.14"
  "${runtime}/libkitty_py/glue.py"
)
for path in "${required[@]}"; do
  if [[ ! -e "${path}" ]]; then
    echo "Release runtime is missing ${path}" >&2
    exit 1
  fi
done

if find "${runtime}" -type f -print0 \
    | xargs -0 strings 2>/dev/null \
    | grep -Fq "/Users/ethanabbate/Desktop/System/home-kitmux"; then
  echo "Release runtime still contains a developer checkout path." >&2
  exit 1
fi

for elf in \
  "${runtime}/bin/linux_session_stress" \
  "${runtime}/lib/libkitty.so" \
  "${runtime}/kitty/fast_data_types.so" \
  "${runtime}/kitty/glfw-x11.so" \
  "${runtime}/kitty/glfw-wayland.so"; do
  file "${elf}" | grep -q "ELF"
  if ldd "${elf}" | grep -q "not found"; then
    echo "Unresolved ELF dependency in ${elf}:" >&2
    ldd "${elf}" >&2
    exit 1
  fi
done

libpython_path="$(
  ldd "${runtime}/lib/libkitty.so" \
    | awk '/libpython3[.]14[.]so[.]1[.]0/ {print $3}'
)"
case "${libpython_path}" in
  "${runtime}/lib/"*) ;;
  *)
    echo "libkitty resolved Python outside the release tree: ${libpython_path}" >&2
    exit 1
    ;;
esac

test_home="/tmp/kitmux-release-home-${UID}"
mkdir -p "${test_home}"
env -i \
  HOME="${test_home}" \
  LANG=C.UTF-8 \
  PATH=/usr/bin:/bin \
  PYTHONHOME="${runtime}" \
  KITTY_SRC="${runtime}" \
  LIBKITTY_PY="${runtime}/libkitty_py" \
  LIBKITTY_TEST_CONFIG="${runtime}/etc/kitty.conf" \
  "${runtime}/bin/linux_session_stress"

echo "Relocatable release runtime: OK (${runtime})"
