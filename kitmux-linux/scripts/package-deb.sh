#!/usr/bin/env bash
set -Eeuo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Run this packager inside a Linux build VM or CI runner." >&2
  exit 1
fi
if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "dpkg-deb is required." >&2
  exit 1
fi

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
workspace="$(cd -- "${script_dir}/.." && pwd)"
linux_root="$(cd -- "${workspace}/.." && pwd)"
output="${1:?usage: package-deb.sh OUTPUT [RUNTIME]}"
runtime="${2:-}"
version="${KITMUX_PACKAGE_VERSION:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' "${workspace}/rust/app/Cargo.toml" | head -1)}"
epoch="${SOURCE_DATE_EPOCH:-0}"
architecture="$(dpkg --print-architecture)"
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/kitmux-deb.XXXXXX")"
cleanup() {
  rm -rf -- "${temporary_root}"
}
trap cleanup EXIT

if [[ -z "${version}" || ! "${version}" =~ ^[0-9A-Za-z.+~-]+$ ]]; then
  echo "Invalid package version: ${version}" >&2
  exit 1
fi
if [[ -e "${output}" ]]; then
  echo "Refusing to replace existing Debian package: ${output}" >&2
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

package_root="${temporary_root}/package"
install -d \
  "${package_root}/DEBIAN" \
  "${package_root}/usr/lib/kitmux" \
  "${package_root}/usr/bin" \
  "${package_root}/usr/share/applications" \
  "${package_root}/usr/share/doc/kitmux"
cp -a "${runtime}/." "${package_root}/usr/lib/kitmux/"
# Release runtimes may be private build directories; installed users need to
# traverse the package root even though the runtime contents remain unchanged.
chmod 0755 "${package_root}/usr/lib/kitmux"
ln -s /usr/lib/kitmux/bin/kitmux "${package_root}/usr/bin/kitmux"
ln -s /usr/lib/kitmux/bin/kitmuxctl "${package_root}/usr/bin/kitmuxctl"
install -m 0644 "${linux_root}/LICENSE" "${package_root}/usr/share/doc/kitmux/copyright"

cat >"${package_root}/DEBIAN/control" <<EOF
Package: kitmux
Version: ${version}
Section: utils
Priority: optional
Architecture: ${architecture}
Maintainer: Kitmux maintainers <maintainers@kitmux.invalid>
Depends: libc6, libgtk-4-1, libepoxy0, libpango-1.0-0, libgdk-pixbuf-2.0-0, libgl1, libxkbcommon0, libxkbcommon-x11-0, libdbus-1-3
Description: native terminal workspace
 Kitmux is a GTK terminal workspace built around the Kitty terminal engine.
EOF
cat >"${package_root}/usr/share/applications/kitmux.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Kitmux
Comment=Native terminal workspace
Exec=/usr/bin/kitmux
Icon=utilities-terminal
Terminal=false
Categories=System;TerminalEmulator;
EOF

find "${package_root}" -exec touch -h -d "@${epoch}" {} +
mkdir -p "$(dirname -- "${output}")"
SOURCE_DATE_EPOCH="${epoch}" dpkg-deb --root-owner-group --build \
  "${package_root}" "${output}" >/dev/null

dpkg-deb --info "${output}" | sed -n '1,14p'
sha256sum "${output}"
echo "Reproducible Kitmux Debian package: ${output}"
