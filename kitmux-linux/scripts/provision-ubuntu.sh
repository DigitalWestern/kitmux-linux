#!/usr/bin/env bash
set -euo pipefail

if [[ "$(id -u)" -eq 0 ]]; then
  sudo=()
else
  sudo=(sudo)
fi

"${sudo[@]}" apt-get update
"${sudo[@]}" env DEBIAN_FRONTEND=noninteractive apt-get install -y \
  binutils build-essential cmake file git golang-go less patchelf podman pkg-config \
  python3-dev zsh \
  libdbus-1-dev libfontconfig-dev libgl1-mesa-dev libwayland-dev \
  libx11-xcb-dev libxcursor-dev libxi-dev libxinerama-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libxrandr-dev wayland-protocols

if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain 1.97.1
fi

# rustup's installer updates PATH only for new shells.  Continue provisioning
# this shell explicitly so a clean VM does not stop before the locked toolchain
# check.
if [[ -f "${HOME}/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "${HOME}/.cargo/env"
fi
rustup toolchain install 1.97.1 --profile minimal
rustup default 1.97.1
rustup component add rustfmt clippy --toolchain 1.97.1
