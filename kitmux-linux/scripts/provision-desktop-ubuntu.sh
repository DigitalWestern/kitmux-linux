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
  ibus-m17n \
  xfce4 \
  xfce4-terminal \
  xdotool \
  xwayland

# XFCE asks polkit for permission to create a colord-managed device on every
# session start. In a headless VNC session nobody answers, and the dialog
# floats over the windows the desktop gate screenshots as evidence. Grant it
# to local sessions so no prompt appears.
sudo tee /etc/polkit-1/rules.d/49-kitmux-colord.rules >/dev/null <<'RULES'
polkit.addRule(function (action, subject) {
  if (action.id.indexOf("org.freedesktop.color-manager.") === 0) {
    return polkit.Result.YES;
  }
});
RULES
