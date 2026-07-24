# Linux engine system dependencies

The engine bundle carries CPython, Kitty, and the transitive closure of Kitty's
pinned non-platform shared libraries. It intentionally uses the distribution's
glibc, ELF loader, X11, XCB, D-Bus, and UUID libraries. The dependency closure
is recorded in `RUNTIME_DEPENDENCIES.json`; an undeclared ELF dependency fails
the release build.

Clean-runtime checks currently install:

| Capability | Ubuntu 26.04 | Fedora 44 |
| --- | --- | --- |
| X11 client | `libx11-6`, `libx11-xcb1`, `libxcursor1` | `libX11`, `libX11-xcb`, `libXcursor` |
| XCB | `libxcb1`, `libxcb-xkb1` | `libxcb` |
| Keyboard | `libxkbcommon0`, `libxkbcommon-x11-0` | `libxkbcommon`, `libxkbcommon-x11` |
| D-Bus | `libdbus-1-3` | `dbus-libs` |
| UUID | `libuuid1` | `libuuid` |

Kitty selects graphics and desktop-integration libraries at runtime with
`dlopen`, so they do not appear in ELF `DT_NEEDED` records. The production GTK
host must provide an OpenGL or EGL implementation and the display backend's
normal GTK dependencies. Optional Kitty integrations such as libcanberra,
startup-notification, systemd, Vulkan, or OSMesa are not requirements for the
terminal-first engine runtime.

This declaration is part of the runtime audit. An unexpected unresolved ELF
dependency fails `test-release-runtime.sh`.
