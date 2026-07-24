# ADR 0002: GTK 4 gets one bounded rendering spike

Status: accepted

GTK 4 is the first toolkit candidate. It wins only if one `GtkGLArea` can host
a real libkitty session with correct OpenGL lifetime, continuous PTY pumping,
keyboard/IME, pointer coordinates, clipboard, focus, and fractional scale on
Wayland and X11.

The spike is disposable and contains no production navigation UI. A failure
against a written criterion triggers one equivalent Qt 6 spike; the project
does not maintain both toolkit implementations.

