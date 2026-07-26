# ADR 0002: GTK 4 is the Linux UI toolkit

Status: accepted

GTK 4 is the Linux UI toolkit. It earned that selection when the bounded Phase
2 spike passed its rendering, input/IME, native Wayland, WebKit coexistence,
fractional scaling, event-fairness, relocatable-loader, and accessibility
viability gates. Selection, clipboard, paste, mouse, wheel, and search are
product work in Phase 4 rather than toolkit kill tests.

The spike is disposable and contains no production navigation UI. A failure
against a written criterion triggers one equivalent Qt 6 spike; the project
does not maintain both toolkit implementations.

## 2026-07-24 Slice 2.1 finding

The first real `GtkGLArea` host passed over X11 with Mesa llvmpipe: one live
libkitty shell rendered, PTY output arrived through a GLib file-descriptor
source, two framebuffer resizes applied, tracked OpenGL state restored after
draw, and close reaped the terminal child. This closes only the rendering and
lifecycle slice; GTK is not chosen until input, IME, Wayland, scale, fairness,
and the bounded WebKit conflict checks pass. At that checkpoint input/IME,
native Wayland, WebKit coexistence, scaling, and fairness were still open.

## 2026-07-25 scope correction

Slices 2.2C through 2.2E — selection, clipboard, safe paste, mouse, wheel, and
search — were removed from this spike and moved to Phase 4 Slice 4.2.

They failed the test this ADR sets. A kill spike answers "can the toolkit do
this at all?"; VTE already ships every one of those behaviors on GTK, so the
answer was known and the slices were pure implementation cost. Meanwhile the
four checks that plausibly *could* disqualify GTK — native Wayland, WebKitGTK
coexistence, fractional and mixed-monitor scaling, and main-loop fairness —
were queued behind them, and would have been discovered only after several
sessions of work that a failure would discard.

The spike now runs those four first, as Slices 2.2C through 2.2F. Slices 2.2A
and 2.2B stay in scope: whether GTK's input model could drive Kitty's keyboard
protocol, including a real input method, was a genuine open question, and the
answer turned out to be yes only after non-obvious work.

"Disposable" also needed qualification. See ADR 0007: the display-free key
translation and its fixtures are durable and stay C behind FFI; only
`src/gtk_terminal_host.c` is disposable.

## 2026-07-25 native Wayland result

Slice 2.2C passed. Weston 14.0.2 ran without Xwayland, and the existing host
reported `GdkWaylandDisplay` before rendering a live libkitty recorder
session. Three exact framebuffer sizes, tracked GL-state restoration,
press/release/repeat, one real IBus preedit/direct commit, and clean child
reaping all passed.

Weston was nested with its X11 backend in the dedicated VNC development
desktop so the result stayed visible. XTEST targeted only the compositor's
outer window; Weston converted those events to `wl_keyboard` events for the
native client. This closes the GTK Wayland-client kill check but does not
claim a physical Wayland desktop, libinput/evdev injection, GPU performance,
or driver behavior.

## 2026-07-25 WebKitGTK coexistence result

Slice 2.2D passed with WebKitGTK 2.52.3. The disposable host mapped one
`WebKitWebView` containing static in-memory HTML beside the live terminal
under X11 and native Wayland. Kitty's tracked GL state restored correctly,
focus entered WebKit without leaking bytes to the terminal and returned
cleanly, and neither run reported load failure or web-process termination.

The loader check preserved this ADR's known-fragile boundary: the host's 139
resolved native libraries included the isolated pinned `libpython`, while
WebKitGTK, JavaScriptCoreGTK, GTK, and their native dependencies came from the
Ubuntu system paths. Kitty's private development-library directory did not
enter the process closure. The VM install added 43 packages and 166 MB, which
is a dependency-cost warning for future packaging rather than a package-size
measurement. This closes the coexistence kill check only; it does not select
GTK, implement browser behavior, or prove a distributable layout.

## 2026-07-25 fractional scaling result

Slice 2.2E passed under nested Sway 1.11 with virtual outputs at 100%, 150%,
and 200%. The compositor advertised `wp_fractional_scale_manager_v1` and
`wp_viewporter`. GTK's double surface scale reported `1/1.5/2`, while the
`GtkGLArea` backing buffer used integer factors `1/2/2`; those values serve
different coordinate systems and must remain separate.

A hash-locked Linux overlay adds a narrow libkitty backing-scale setter. It
rebuilds the shared font atlas at the new DPI while retaining the configured
point size and live sessions. One recorder session moved through all three
outputs and back to 100%, with exact framebuffer, cell, grid, content, and
child-PID assertions. Its original 100% metrics returned exactly.

This closes the fractional and unlike-output correctness check. It does not
claim physical mixed-DPI hardware or GPU/compositor performance.

## 2026-07-26 event-fairness result

Slice 2.2F passed after the spike exposed and fixed starvation at the shared
source priority. Five continuously readable PTYs at `G_PRIORITY_DEFAULT`
allowed the heartbeat to run but starved every frame and resize. Moving only
the PTY sources to `G_PRIORITY_DEFAULT_IDLE` preserved progress for every
session while 60 resizes, more than 700 frames, and bounded heartbeat, frame,
and pump latency passed during a 12-second X11/llvmpipe flood.

This is scheduler-fairness evidence, not physical-GPU performance evidence.

## 2026-07-26 toolkit decision

Slice 2.3 selected GTK 4. A temporary installed GUI layout placed the host in
`bin/` and `libkitty` plus the pinned `libpython` in `lib/`. `$ORIGIN`-relative
runpaths loaded that tree and distro GTK/WebKit dependencies without a broad
`LD_LIBRARY_PATH`, rendered a live frame, and reaped the child cleanly.

The terminal surface exposes GTK's terminal accessibility role and label,
accepts focus, transfers focus to an ordinary text control and back, and keeps
the proven `GtkIMMulticontext` input path. This establishes the required
accessibility focus and text-input viability; full AT-SPI screen-reader and
terminal-content coverage remains Phase 5 work.

No written GTK decision criterion failed, so the conditional Qt 6 probe was
not run. Physical GPU and mixed-monitor hardware proof remain Phase 6 gates,
and native packaging/clean-install proof remains later release work.

## Loader boundary retained

The spike also established a loader boundary for the eventual native package.
The GTK process must not add Kitty's full private dependency directory to its
global loader path because it shadows distro copies of Cairo, HarfBuzz,
GLib, and xkbcommon. The development host instead places only the pinned
`libpython` in an isolated directory and lets the GTK process use the distro's
ABI-compatible native libraries. Any self-contained GUI package must preserve
that separation or introduce a genuinely isolated engine process; it must not
reintroduce a broad `LD_LIBRARY_PATH`.
