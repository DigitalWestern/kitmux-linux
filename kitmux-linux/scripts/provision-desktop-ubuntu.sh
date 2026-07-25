#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Run this script inside the Ubuntu development VM." >&2
  exit 1
fi

sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
  build-essential \
  cmake \
  dbus-x11 \
  ibus \
  ibus-gtk4 \
  libgtk-4-dev \
  libxtst-dev \
  mesa-utils \
  ninja-build \
  novnc \
  pkg-config \
  scrot \
  tigervnc-standalone-server \
  tigervnc-tools \
  websockify \
  weston \
  xfce4 \
  xfce4-terminal \
  xdotool \
  xwayland
