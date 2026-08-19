#!/usr/bin/env bash
set -Eeuo pipefail

if [[ "$(uname -s)" != "Linux" || -z "${DISPLAY:-}" ]]; then
  echo "Run this gate inside a Linux VM with DISPLAY set." >&2
  exit 1
fi
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "${script_dir}/gate-common.sh"

case "$#" in
  0) build_packages=1; tarball=""; deb="" ;;
  2) build_packages=0; tarball="$1"; deb="$2" ;;
  *) echo "usage: test-package-lifecycle.sh [TARBALL DEB]" >&2; exit 2 ;;
esac
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/kitmux-package-lifecycle.XXXXXX")"
tar_install="${temporary_root}/tar-install"
tar_state="${temporary_root}/tar-state"
deb_state="${temporary_root}/deb-state"
upgrade_deb="${temporary_root}/kitmux-upgrade.deb"
active_pid=""
cleanup() {
  if [[ -n "${active_pid}" ]] && kill -0 "${active_pid}" 2>/dev/null; then
    kill -TERM "${active_pid}" 2>/dev/null || true
    for _ in $(seq 1 30); do
      kill -0 "${active_pid}" 2>/dev/null || break
      sleep 0.1
    done
    kill -KILL "${active_pid}" 2>/dev/null || true
  fi
  if dpkg-query -W -f='${Status}' kitmux 2>/dev/null | grep -q 'install ok installed'; then
    sudo -n dpkg -P kitmux >/dev/null 2>&1 || true
  fi
  rm -rf -- "${temporary_root}" 2>/dev/null || true
}
trap cleanup EXIT

if [[ "${build_packages}" == "1" ]]; then
  runtime="${temporary_root}/runtime"
  tarball="${temporary_root}/kitmux.tar.xz"
  deb="${temporary_root}/kitmux.deb"
  KITMUX_REQUIRE_DURABLE_INPUTS=1 KITMUX_ALLOW_SOURCE_DEPENDENCY_BUILD=1 \
    KITMUX_BUILD_APP_RUNTIME=1 \
    KITMUX_APP_TEST_HOOKS=OFF "${script_dir}/build-release-runtime.sh" "${runtime}"
  KITMUX_PACKAGE_VERSION=0.1.0 "${script_dir}/package-tarball.sh" "${tarball}" "${runtime}"
  KITMUX_PACKAGE_VERSION=0.1.0 "${script_dir}/package-deb.sh" "${deb}" "${runtime}"
fi

dpkg_install() {
  sudo -n dpkg -i "$1" >/dev/null
}

launch_and_stop() {
  local binary="$1"
  local state_root="$2"
  local log="${state_root}/launch.log"
  local socket="${state_root}/kitmux.sock"
  mkdir -m 700 -p "${state_root}" "${state_root}/config" "${state_root}/state" \
    "${state_root}/data" "${state_root}/cache"
  chmod 700 "${state_root}"
  env -i \
    DISPLAY="${DISPLAY}" HOME="${HOME}" LANG=C.UTF-8 PATH=/usr/bin:/bin \
    GSK_RENDERER=gl GTK_IM_MODULE=gtk-im-context-simple GTK_USE_PORTAL=0 GIO_USE_PORTAL=0 \
    KITMUX_SOCKET_PATH="${socket}" \
    XDG_CONFIG_HOME="${state_root}/config" \
    XDG_STATE_HOME="${state_root}/state" \
    XDG_DATA_HOME="${state_root}/data" \
    XDG_CACHE_HOME="${state_root}/cache" \
    "${binary}" >"${log}" 2>&1 &
  active_pid=$!
  for _ in $(seq 1 300); do
    grep -q '^kitmux event=control_server_ready ' "${log}" && break
    if ! kill -0 "${active_pid}" 2>/dev/null; then
      cat "${log}" >&2
      return 1
    fi
    sleep 0.1
  done
  grep -q '^kitmux event=control_server_ready ' "${log}"
  kill -TERM "${active_pid}"
  wait "${active_pid}"
  grep -q '^kitmux event=sigterm_shutdown' "${log}"
  active_pid=""
}

mkdir -p "${tar_install}"
tar -xJf "${tarball}" -C "${tar_install}"
tar_binary="$(find "${tar_install}" -path '*/bin/kitmux' -type f -perm -111 -print -quit)"
[[ -n "${tar_binary}" ]]
launch_and_stop "${tar_binary}" "${tar_state}"
echo "tarball install and launch: OK"

dpkg-deb --info "${deb}" >/dev/null
dpkg_install "${deb}"
[[ "$(dpkg-query -W -f='${Version}' kitmux)" == "0.1.0" ]]
[[ -x /usr/bin/kitmux && -x /usr/bin/kitmuxctl ]]
[[ -f /usr/share/applications/kitmux.desktop ]]
launch_and_stop /usr/bin/kitmux "${deb_state}"
echo "Debian install and launch: OK"

dpkg-deb --raw-extract "${deb}" "${temporary_root}/upgrade-root"
sed -i 's/^Version: .*/Version: 0.1.1/' \
  "${temporary_root}/upgrade-root/DEBIAN/control"
dpkg-deb --build --root-owner-group "${temporary_root}/upgrade-root" \
  "${upgrade_deb}" >/dev/null
dpkg_install "${upgrade_deb}"
[[ "$(dpkg-query -W -f='${Version}' kitmux)" == "0.1.1" ]]
echo "upgrade: OK"

dpkg_install "${deb}"
[[ "$(dpkg-query -W -f='${Version}' kitmux)" == "0.1.0" ]]
echo "downgrade: OK"

dpkg_install "${deb}"
[[ "$(dpkg-query -W -f='${Version}' kitmux)" == "0.1.0" ]]
echo "reinstall: OK"

sudo -n dpkg -r kitmux >/dev/null
! dpkg-query -W -f='${Status}' kitmux 2>/dev/null | grep -q 'install ok installed'
[[ ! -e /usr/bin/kitmux && ! -e /usr/bin/kitmuxctl ]]
[[ ! -e /usr/lib/kitmux && ! -e /usr/share/applications/kitmux.desktop ]]
echo "uninstall: OK"
echo "Package lifecycle on fresh Ubuntu ARM64 VM: OK"
