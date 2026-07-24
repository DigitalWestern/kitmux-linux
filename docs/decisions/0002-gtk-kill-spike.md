# ADR 0002: GTK 4 gets one bounded rendering spike

Status: accepted

GTK 4 is the first toolkit candidate. It wins only if one `GtkGLArea` can host
a real libkitty session with correct OpenGL lifetime, continuous PTY pumping,
keyboard/IME, pointer coordinates, clipboard, focus, and fractional scale on
Wayland and X11.

The spike is disposable and contains no production navigation UI. A failure
against a written criterion triggers one equivalent Qt 6 spike; the project
does not maintain both toolkit implementations.

## 2026-07-24 Slice 2.1 finding

The first real `GtkGLArea` host passed over X11 with Mesa llvmpipe: one live
libkitty shell rendered, PTY output arrived through a GLib file-descriptor
source, two framebuffer resizes applied, tracked OpenGL state restored after
draw, and close reaped the terminal child. This closes only the rendering and
lifecycle slice; GTK is not chosen until input, IME, Wayland, scale, fairness,
and the bounded WebKit conflict checks pass.

The spike also established a loader boundary for the eventual native package.
The GTK process must not add Kitty's full private dependency directory to its
global loader path because it shadows distro copies of Cairo, HarfBuzz,
GLib, and xkbcommon. The development host instead places only the pinned
`libpython` in an isolated directory and lets the GTK process use the distro's
ABI-compatible native libraries. Any self-contained GUI package must preserve
that separation or introduce a genuinely isolated engine process; it must not
reintroduce a broad `LD_LIBRARY_PATH`.
