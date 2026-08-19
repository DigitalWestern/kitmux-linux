#!/usr/bin/env bash
set -euo pipefail

runtime="${1:?usage: test-release-runtime.sh /path/to/runtime}"
runtime="$(realpath "${runtime}")"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "${script_dir}/gate-common.sh"
component_manifest="${script_dir}/../release/runtime-components.json"

mapfile -t python_dirs < <(
  find "${runtime}/lib" -mindepth 1 -maxdepth 1 -type d -name 'python3.*' -print
)
mapfile -t libpython_files < <(
  find "${runtime}/lib" -mindepth 1 -maxdepth 2 -type f \
    -name 'libpython3.*.so.1.0' -print
)
mapfile -t libkitty_files < <(
  find "${runtime}/lib" -mindepth 1 -maxdepth 2 -type f \
    -name 'libkitty.so' -print
)
if [[ "${#python_dirs[@]}" -ne 1 || "${#libpython_files[@]}" -ne 1 \
    || "${#libkitty_files[@]}" -ne 1 ]]; then
  echo "Release runtime must contain exactly one Python library and standard library." >&2
  exit 1
fi
python_dir="${python_dirs[0]}"
libpython_file="${libpython_files[0]}"
libkitty_file="${libkitty_files[0]}"

required=(
  "${runtime}/bin/linux_session_stress"
  "${runtime}/etc/kitty.conf"
  "${runtime}/kitty/fast_data_types.so"
  "${libkitty_file}"
  "${libpython_file}"
  "${python_dir}"
  "${runtime}/libkitty_py/glue.py"
  "${runtime}/share/RUNTIME_DEPENDENCIES.json"
  "${runtime}/share/SHA256SUMS"
  "${runtime}/share/kitmux-engine.spdx.json"
  "${runtime}/share/runtime-components.json"
)
if [[ -x "${runtime}/bin/kitmux" ]]; then
  required+=("${runtime}/bin/kitmuxctl")
fi
for path in "${required[@]}"; do
  if [[ ! -e "${path}" ]]; then
    echo "Release runtime is missing ${path}" >&2
    exit 1
  fi
done

mapfile -t forbidden_slang < <(
  find "${runtime}/lib" \( -type f -o -type l \) -iname 'libslang*' -print
)
if [[ "${#forbidden_slang[@]}" -ne 0 ]]; then
  printf 'Release runtime contains unreferenced libslang payload:\n%s\n' \
    "${forbidden_slang[@]}" >&2
  exit 1
fi
runtime_kib="$(du -sk -- "${runtime}" | awk '{print $1}')"
echo "Release runtime size: ${runtime_kib} KiB; libslang payload: 0 bytes"

mapfile -d '' -t elf_files < <(
  while IFS= read -r -d '' candidate; do
    if file "${candidate}" | grep -q 'ELF'; then
      printf '%s\0' "${candidate}"
    fi
  done < <(find "${runtime}" -type f -print0)
)
for elf in "${elf_files[@]}"; do
  if strings "${elf}" 2>/dev/null \
      | grep -Eq '(/Users/[^/]+/|/home/[^/]+/(Desktop|code|projects|src|work)/|/work/kitmux|home-kitmux)'; then
    echo "Release ELF contains a developer checkout path: ${elf}" >&2
    strings "${elf}" 2>/dev/null \
      | grep -E '(/Users/[^/]+/|/home/[^/]+/(Desktop|code|projects|src|work)/|/work/kitmux|home-kitmux)' >&2
    exit 1
  fi
  if ldd "${elf}" | grep -q "not found"; then
    echo "Unresolved ELF dependency in ${elf}:" >&2
    ldd "${elf}" >&2
    exit 1
  fi
done

libpython_path="$(
  ldd "${libkitty_file}" \
    | awk '/libpython3[.][0-9]+[.]so[.]1[.]0/ {print $3}'
)"
case "${libpython_path}" in
  "${runtime}/lib/"*) ;;
  *)
    echo "libkitty resolved Python outside the release tree: ${libpython_path}" >&2
    exit 1
    ;;
esac

test_home="$(mktemp -d /tmp/kitmux-release-home.XXXXXX)"
cleanup() {
  rm -rf -- "${test_home}"
}
trap cleanup EXIT
env -i \
  HOME="${test_home}" \
  LANG=C.UTF-8 \
  PATH=/usr/bin:/bin \
  PYTHONHOME="${runtime}" \
  KITTY_SRC="${runtime}" \
  LIBKITTY_PY="${runtime}/libkitty_py" \
  LIBKITTY_TEST_CONFIG="${runtime}/etc/kitty.conf" \
  "${runtime}/bin/linux_session_stress"

python3 "${script_dir}/release-tools.py" verify \
  --runtime "${runtime}" \
  --manifest "${component_manifest}"
(
  cd "${runtime}"
  sha256sum --check --strict --quiet share/SHA256SUMS
)

echo "Relocatable release runtime: OK (${runtime})"
