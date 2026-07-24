# Linux engine system dependencies

The proof bundle carries CPython, Kitty, and Kitty's pinned non-platform shared
libraries. It intentionally uses the distribution's glibc, ELF loader, X11,
XCB, and D-Bus libraries. The later GTK application will add GTK, OpenGL, and
Wayland package requirements.

Clean-runtime checks currently install:

| Capability | Ubuntu 26.04 | Fedora 44 |
| --- | --- | --- |
| X11 client | `libx11-6`, `libx11-xcb1`, `libxcursor1` | `libX11`, `libX11-xcb`, `libXcursor` |
| XCB | `libxcb1`, `libxcb-xkb1` | `libxcb` |
| Keyboard | `libxkbcommon0`, `libxkbcommon-x11-0` | `libxkbcommon`, `libxkbcommon-x11` |
| D-Bus | `libdbus-1-3` | `dbus-libs` |

This declaration is part of the runtime audit. An unexpected unresolved ELF
dependency fails `test-release-runtime.sh`.
