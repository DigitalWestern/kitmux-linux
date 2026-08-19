#!/usr/bin/env bash
set -Eeuo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Run this packager inside a Linux build VM or CI runner." >&2
  exit 1
fi

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
workspace="$(cd -- "${script_dir}/.." && pwd)"
linux_root="$(cd -- "${workspace}/.." && pwd)"
output="${1:?usage: package-tarball.sh OUTPUT [RUNTIME]}"
runtime="${2:-}"
version="${KITMUX_PACKAGE_VERSION:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' "${workspace}/rust/app/Cargo.toml" | head -1)}"
epoch="${SOURCE_DATE_EPOCH:-0}"
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/kitmux-tarball.XXXXXX")"
cleanup() {
  rm -rf -- "${temporary_root}"
}
trap cleanup EXIT

if [[ -z "${version}" || ! "${version}" =~ ^[0-9A-Za-z.+~-]+$ ]]; then
  echo "Invalid package version: ${version}" >&2
  exit 1
fi
if [[ -e "${output}" ]]; then
  echo "Refusing to replace existing tarball: ${output}" >&2
  exit 1
fi
if [[ -z "${runtime}" ]]; then
  runtime="${temporary_root}/runtime"
  KITMUX_BUILD_APP_RUNTIME=1 KITMUX_APP_TEST_HOOKS=OFF \
    "${script_dir}/build-release-runtime.sh" "${runtime}"
fi
if [[ ! -x "${runtime}/bin/kitmux" || ! -x "${runtime}/bin/kitmuxctl" ]]; then
  echo "Runtime is missing the release app and CLI: ${runtime}" >&2
  exit 1
fi

package_root="${temporary_root}/kitmux-${version}"
mkdir -p "${package_root}/share"
cp -a "${runtime}/." "${package_root}/"
install -m 0644 "${linux_root}/LICENSE" "${package_root}/share/LICENSE"
install -m 0644 "${workspace}/README.md" "${package_root}/share/README.md"

find "${package_root}" -exec touch -h -d "@${epoch}" {} +
mkdir -p "$(dirname -- "${output}")"
tar --sort=name --mtime="@${epoch}" --owner=0 --group=0 --numeric-owner \
  --format=gnu -cJf "${output}" \
  -C "${temporary_root}" "kitmux-${version}"

sha256sum "${output}"
echo "Reproducible Kitmux tarball: ${output}"
