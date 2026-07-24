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
case "$(uname -m)" in
  aarch64|arm64) kitty_platform="linux-arm64" ;;
  x86_64|amd64) kitty_platform="linux-64" ;;
  *)
    echo "Unsupported Linux architecture: $(uname -m)" >&2
    exit 1
    ;;
esac
dependencies="${kitty_root}/dependencies/${kitty_platform}"
build_dir="${workspace}/build-release"
output="${1:-${workspace}/build/kitmux-engine-runtime}"
component_manifest="${workspace}/release/runtime-components.json"

python3 "${script_dir}/release-tools.py" verify-inputs \
  --linux-root "${linux_root}" \
  --platform "${kitty_platform}" \
  --manifest "${component_manifest}"

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
mkdir -p "$(dirname -- "${output}")"

mapfile -t python_roots < <(
  find "${dependencies}/lib" -mindepth 1 -maxdepth 1 -type d -name 'python3.*' -print
)
if [[ "${#python_roots[@]}" -ne 1 ]]; then
  echo "Expected one Python standard library under ${dependencies}/lib." >&2
  printf 'Found: %s\n' "${python_roots[*]:-(none)}" >&2
  exit 1
fi
python_root="${python_roots[0]}"
python_dir="$(basename -- "${python_root}")"

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
cp -a "${python_root}" "${staging}/lib/"
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
install -m 0644 \
  "${workspace}/release/SYSTEM_DEPENDENCIES.md" \
  "${staging}/share/"
install -m 0644 \
  "${component_manifest}" \
  "${staging}/share/runtime-components.json"
cp -a "${workspace}/release/licenses" "${staging}/share/"

# Keep the Python runtime and Kitty assets needed by libkitty, not their build
# metadata, caches, C sources, headers, or developer-only launcher executable.
rm -rf -- "${staging}/lib/${python_dir}/site-packages"
find "${staging}/lib/${python_dir}" -mindepth 1 -maxdepth 1 \
  -type d -name 'config-*' -prune -exec rm -rf -- {} +
find "${staging}/kitty" -type f \
  ! -name '*.py' ! -name '*.pyi' ! -name '*.so' ! -name '*.glsl' -delete
find "${staging}/kitty" -depth -type d -empty -delete

find "${staging}" -type d -name __pycache__ -prune -exec rm -rf -- {} +
find "${staging}" -type f -name '*.pyc' -delete

mapfile -d '' -t dependency_roots < <(
  printf '%s\0' "${staging}/lib/libkitty.so"
  find "${staging}/kitty" -type f -name '*.so' -print0
  find "${staging}/lib/${python_dir}/lib-dynload" -type f -name '*.so' -print0
)
dependency_arguments=()
for root in "${dependency_roots[@]}"; do
  dependency_arguments+=(--root "${root}")
done
python3 "${script_dir}/release-tools.py" copy-dependencies \
  --dependency-lib "${dependencies}/lib" \
  --runtime-lib "${staging}/lib" \
  --report "${staging}/share/RUNTIME_DEPENDENCIES.json" \
  "${dependency_arguments[@]}"

# Some upstream archives store shared objects without an executable bit.
# Linux can load them, but tooling such as Fedora's ldd warns; normalize every
# shipped ELF to the conventional runtime mode.
while IFS= read -r -d '' candidate; do
  if file "${candidate}" | grep -q 'ELF'; then
    chmod 0755 "${candidate}"
  fi
done < <(find "${staging}" -type f -print0)

while IFS= read -r -d '' elf; do
  if file "${elf}" | grep -q "ELF .* shared object"; then
    patchelf --set-rpath '$ORIGIN' "${elf}"
  fi
done < <(find "${staging}/lib" -maxdepth 1 -type f -print0)

while IFS= read -r -d '' elf; do
  patchelf --set-rpath '$ORIGIN/../..' "${elf}"
done < <(
  find "${staging}/lib/${python_dir}/lib-dynload" \
    -type f -name '*.so' -print0
)

for elf in "${staging}/kitty/"*.so; do
  patchelf --set-rpath '$ORIGIN/../lib' "${elf}"
done
patchelf --set-rpath '$ORIGIN/../lib' \
  "${staging}/bin/linux_session_stress"

python3 "${script_dir}/release-tools.py" generate-sbom \
  --runtime "${staging}" \
  --manifest "${component_manifest}" \
  --output "${staging}/share/kitmux-engine.spdx.json"

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
