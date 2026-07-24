#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Run this script inside the Ubuntu development VM." >&2
  exit 1
fi

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
workspace="$(cd -- "${script_dir}/.." && pwd)"
linux_root="$(cd -- "${workspace}/.." && pwd)"
kitty_root="${linux_root}/.source/kitty"
reference_root="${linux_root}/.source/reference"
dependencies="${kitty_root}/dependencies/linux-arm64"
build_dir="${workspace}/build-release"
output="${1:-${workspace}/build/kitmux-engine-runtime}"

if [[ ! -x "${dependencies}/bin/python" ]] \
    || [[ ! -f "${kitty_root}/kitty/fast_data_types.so" ]]; then
  echo "Build the pinned Kitty development runtime first." >&2
  exit 1
fi
if [[ -e "${output}" ]]; then
  echo "Refusing to replace existing release tree: ${output}" >&2
  echo "Pass a new output path or remove the generated tree explicitly." >&2
  exit 1
fi
install -d "$(dirname -- "${output}")"

env LD_LIBRARY_PATH="${dependencies}/lib" \
cmake -S "${workspace}" -B "${build_dir}" \
  -DCMAKE_BUILD_TYPE=Release \
  -DPython3_ROOT_DIR="${dependencies}" \
  -DPython3_EXECUTABLE="${dependencies}/bin/python" \
  -DPython3_FIND_STRATEGY=LOCATION \
  -DPython3_FIND_UNVERSIONED_NAMES=FIRST
cmake --build "${build_dir}" --parallel

staging="$(mktemp -d "${output}.staging.XXXXXX")"
cleanup() {
  if [[ -d "${staging}" ]]; then
    rm -rf -- "${staging}"
  fi
}
trap cleanup EXIT

install -d \
  "${staging}/bin" \
  "${staging}/etc" \
  "${staging}/fonts" \
  "${staging}/lib" \
  "${staging}/libkitty_py" \
  "${staging}/share"

install -m 0755 "${build_dir}/linux_session_stress" "${staging}/bin/"
install -m 0755 "${build_dir}/libkitty.so" "${staging}/lib/"
cp -a "${dependencies}/lib/"*.so* "${staging}/lib/"
cp -a "${dependencies}/lib/python3.14" "${staging}/lib/"
cp -a "${kitty_root}/kitty" "${staging}/"
cp -a "${reference_root}/libkitty/py/." "${staging}/libkitty_py/"
install -m 0644 \
  "${reference_root}/libkitty/tests/fixtures/kitty.conf" \
  "${staging}/etc/kitty.conf"
install -m 0644 \
  "${kitty_root}/fonts/SymbolsNerdFontMono-Regular.ttf" \
  "${staging}/fonts/"
install -m 0644 \
  "${workspace}/release/THIRD_PARTY.md" \
  "${staging}/share/"

find "${staging}" -type d -name __pycache__ -prune -exec rm -rf -- {} +
find "${staging}" -type f -name '*.pyc' -delete

while IFS= read -r -d '' elf; do
  if file "${elf}" | grep -q "ELF .* shared object"; then
    patchelf --set-rpath '$ORIGIN' "${elf}"
  fi
done < <(find "${staging}/lib" -maxdepth 1 -type f -print0)

while IFS= read -r -d '' elf; do
  patchelf --set-rpath '$ORIGIN/../..' "${elf}"
done < <(
  find "${staging}/lib/python3.14/lib-dynload" \
    -type f -name '*.so' -print0
)

for elf in "${staging}/kitty/"*.so; do
  patchelf --set-rpath '$ORIGIN/../lib' "${elf}"
done
patchelf --set-rpath '$ORIGIN/../lib' \
  "${staging}/bin/linux_session_stress"

(
  cd "${staging}"
  find . -type f ! -path './share/SHA256SUMS' -print0 \
    | sort -z \
    | xargs -0 sha256sum >share/SHA256SUMS
)

"${script_dir}/test-release-runtime.sh" "${staging}"
mv -- "${staging}" "${output}"
trap - EXIT
"${script_dir}/test-release-runtime.sh" "${output}"

echo "Release runtime created at ${output}"
