# Kitmux Linux next steps

Start with Slice 2.2E — **scaling**. Slices 2.2A through 2.2D are closed.

Phase 2 was reordered on 2026-07-25. Slice 2.2C is no longer selection and
clipboard: those, along with mouse, wheel, and search, moved to Phase 4 as
product work. The remaining Phase 2 slices are the checks that could actually
disqualify GTK, cheapest and most dangerous first. The reasoning is in
[Why Slice 2.2 was cut short](LINUX_PORT_PLAN.md#why-slice-22-was-cut-short)
and ADR 0007.

Phase 0.4's portable fixture corpus and Phase 3 Slices 3.1 and 3.2 are closed.
The Rust model now consumes the applicable frozen fixtures and owns the
bounded Linux contract adapters. This file still assigns one Phase 2 slice;
do not start the cross-host Slice 3.3 work incidentally from the Phase 2
handoff.

Do not begin product navigation UI, Phase 3.3 compatibility/import, browser
product functionality, packaging, or the monorepo migration.

## Preconditions

1. Read `PORT_STATUS.md`, this file, the Phase 2 section of
   `LINUX_PORT_PLAN.md`, and ADRs 0006, 0007, and 0008.
2. Confirm `git status --short` is clean and inspect commits newer than the
   Slice 2.2D entry in `PORT_STATUS.md`.
3. Run:

   ```sh
   kitmux-linux/scripts/materialize-reference.sh
   limactl start kitmux-linux
   limactl start kitmux-linux-desktop
   limactl shell kitmux-linux -- \
     "$PWD/kitmux-linux/scripts/test-headless.sh"
   limactl shell kitmux-linux-desktop -- \
     "$PWD/kitmux-linux/scripts/start-desktop.sh"
   limactl shell kitmux-linux-desktop -- \
     "$PWD/kitmux-linux/scripts/test-desktop.sh"
   ```

4. If the baseline fails, repair or record that regression before adding
   behavior.

## Before adding any file: which category is it?

ADR 0007 splits the spike in two. Place new code before writing it.

**Durable** — survives the toolkit decision, stays C, called over FFI by the
future Rust host:

- `src/gtk_key_translation.{c,h}`
- `tests/gtk_key_matrix.c`, `tests/pty_input_recorder.c`,
  `tests/x11_key_injector.c`

Durable code carries no `GdkDisplay` dependency, no widget state, no I/O, and
no global state beyond the caller-owned tracker, and its expectations come
from Kitty's pinned `key_encoding.c` rather than from this host's output.
Those properties are what make it portable; breaking one needs a new ADR.

**Disposable** — replaced wholesale by the Phase 4 application shell:

- `src/gtk_terminal_host.c`, including every `KITMUX_GTK_*` harness variable.

The test: code answering "can this toolkit do X at all?" is spike work. Code
implementing how Kitmux does X is product work and belongs in Phase 4.

## Completed sub-slices

### 2.2A — Deterministic keyboard harness — done 2026-07-25

Exact commands, byte expectations, and limits are in `PORT_STATUS.md`. What
later work inherits:

- `src/gtk_key_translation.{c,h}` is the display-free GDK-to-libkitty key
  translation and held-key tracker. It has no GdkDisplay dependency on
  purpose, so `tests/gtk_key_matrix.c` can assert fixed bytes without a
  display.
- `tests/pty_input_recorder.c` is the child fixture. `KITMUX_RECORDER_INIT`
  puts the terminal into a known live state before any key is sent, and the
  host's first `GTK terminal PTY output:` line is the harness's proof that the
  state reached kitty's Screen.
- `tests/x11_key_injector.c` replaces `xdotool` for key synthesis. Do not go
  back to `xdotool` for function keys; the reason is recorded in
  `PORT_STATUS.md` and in the tool's own header comment. It is X11-only. Slice
  2.2C reused it only against Weston's outer X11 window, with Weston delivering
  native `wl_keyboard` events to the GTK client; that bridge remains
  display-bound evidence.
- The host reports every translated event with its exact bytes, plus focus
  ownership and widget bounds, on stdout. Keep that contract: the desktop gate
  asserts against it.

### 2.2B — Compose, layouts, and IME — done 2026-07-25

Exact commands, byte expectations, and limits are in `PORT_STATUS.md`. What
later work inherits:

- A `GtkIMMulticontext` sees every press first and owns committed text.
  `kitmux_translate_gdk_key` takes that text as an argument and must never go
  back to synthesizing it.
- Two commit routes, both logged: a first single-scalar commit with no
  composition in progress is re-encoded through `kitty_session_encode_key`;
  anything else is written to the child as UTF-8. Keep both — protocol-aware
  applications depend on the first, composition correctness on the second.
- Releases deliberately bypass the input method, and a release whose press
  never reached the terminal as a key event is withheld.
- The desktop gate changes X auto-repeat, the keyboard layout, and the active
  IBus engine while it runs. Anything added to it must restore session state
  on exit. See ADR 0008 R3.

Backend-independent versus desktop-only: the encoder expectations for layouts
and AltGr live in `tests/gtk_key_matrix.c` and need no display. Every Compose,
dead-key, preedit, and commit assertion needs the desktop session, because
only a live `GtkIMContext` produces them.

### 2.2C — Native Wayland client path — done 2026-07-25

- The disposable desktop harness starts Weston 14.0.2 without Xwayland and
  launches the existing host with `GDK_BACKEND=wayland`; the host asserts
  `GdkWaylandDisplay`.
- The live libkitty recorder rendered and resized through three exact
  framebuffer sizes, kept the tracked host GL state intact, and reaped its
  child on close.
- Press, release, compositor-generated repeat, and a real
  `m17n:t:latn-post` preedit/commit flow passed with exact child bytes.
- Weston is nested in the dedicated X11 VNC desktop for visible automation.
  XTEST targets only the compositor's outer window; Weston emits the native
  Wayland input events. This proves GTK's Wayland path, not physical
  libinput/evdev or a general-purpose Wayland injection API.
- The gate and exact limits are recorded in `PORT_STATUS.md`; visible evidence
  is `kitmux-linux/gtk-wayland-proof.png`.

### 2.2D — WebKitGTK conflict probe — done 2026-07-25

A minimal `WebKitWebView` adjacent to the live terminal, in one process. This
is a conflict probe, not browser product work — no navigation, no chrome, no
data session.

Look for, specifically:

- GL context conflicts between WebKit's compositor and `GtkGLArea`;
- loader and symbol collisions against the isolated-`libpython` boundary in
  ADR 0002, which is the known-fragile part;
- focus interference;
- the dependency closure a future package would inherit.

Run under both display backends.

Gate result: clean under X11 and native Wayland. WebKitGTK 2.52.3 loaded an
in-memory fixture beside the live terminal; tracked GL state, focus isolation
and return, exact child bytes, clean process exit, and the isolated-libpython
loader boundary all passed. The host resolved a 139-library native closure.
Exact evidence and limits are in `PORT_STATUS.md`.

This is deliberately early. It is cheap, it is a known-hard interaction, and a
failure changes the toolkit decision. Finding it after Phase 4 exists would be
the most expensive mistake available in this phase.

## Remaining Phase 2 sequence

### 2.2E — Scaling — next

- Coordinates, framebuffer size, cell metrics, and rendered text at 100%, a
  fractional scale, and 200%.
- A live window moved between unlike monitor scales, without session loss or
  cell-metric drift.
- Watch `kitty_render_init`'s scale argument against
  `gtk_widget_get_scale_factor` under fractional scaling. `place_composition`
  in the host already divides by that factor; fractional scale is where GTK
  and Kitty are most likely to disagree, and where an integer scale factor
  stops being the right question.

Gate: exact framebuffer and cell assertions at each scale, plus one scale
change applied to a live session.

### 2.2F — Event fairness

- Heartbeat and frame latency during sustained PTY flood, repeated resize, and
  several hidden sessions pumping at once.
- Confirm the GLib main loop does not starve UI events under output pressure,
  and that `g_unix_fd_add_full`'s current `G_PRIORITY_DEFAULT` is defensible.

Gate: bounded, recorded latency figures under load. Absolute numbers on
llvmpipe are not performance evidence — the claim is fairness, not speed.

### 2.3 — Close Phase 2 and decide

Do not write a permanent GTK selection ADR until all of these have evidence:

1. Slices 2.1, 2.2A, 2.2B, and 2.2C through 2.2F are green.
2. The isolated-libpython boundary works in a release-shaped GUI layout
   without a broad `LD_LIBRARY_PATH`.
3. Required accessibility focus and text-input behavior is viable.

Then update `contracts/feature-inventory.json`, `support-matrix.yml`, ADR
0002, and `PORT_STATUS.md` with exact commands and results.

If a criterion fails for a concrete GTK limitation, run one time-boxed,
equivalent Qt 6 probe. Under ADR 0007 that probe replaces
`gtk_terminal_host.c` and re-targets the durable translation unit's input
struct; it does not restart key semantics. If both toolkits fail the same
renderer boundary, stop and redesign that boundary rather than accumulating
workarounds.

At least one physical Mesa GPU is required before beta but does not block the
toolkit decision. It moved to Phase 6.

## Deferred but explicit

- Selection, clipboard, safe paste, mouse, wheel, and search are Phase 4
  Slice 4.2, not Phase 2.
- Phase 0.4's shared valid/invalid fixture corpus is frozen. Phase 3.1 and
  bounded-contract Slice 3.2 are closed; cross-host compatibility/import
  Slice 3.3 still requires its own explicit assignment.
- Phase 0.6 defines the macOS re-baselining ritual. Reference drift is
  currently unmeasured.
- `contracts/feature-inventory.json` must reach per-behavior granularity
  before Phase 5, or Phase 9's parity gate is unfalsifiable.
- ADR 0008 R1 through R4: standalone buildability, one automated gate, a
  desktop gate that takes a display instead of creating one, and locked
  x86_64 inputs.
- x86_64, GNOME, physical GPU, `.deb`/RPM, clean desktop install,
  upgrade/uninstall, and public-distribution work remain later gates.
- Repository migration is planned in `docs/MONOREPO_MIGRATION.md` and must be
  performed in a separate session.

## Resume prompt

```text
Read AGENTS.md, PORT_STATUS.md, NEXT_STEPS.md, the Phase 2 section of
LINUX_PORT_PLAN.md, and ADRs 0006 through 0008. Note that Phase 2 was
reordered and Slices 2.2C native Wayland and 2.2D WebKitGTK coexistence are
closed. Phase 3 Slices 3.1 and 3.2 are also closed; do not begin Slice 3.3.
Confirm the worktree and both VM baselines. Implement only Slice
2.2E: prove coordinates, framebuffer size, cell metrics, and rendered text at
100%, a fractional scale, and 200%; then apply a scale change to a live
session without session loss or cell-metric drift. Inspect how Kitty's render
scale relates to GTK's integer widget scale factor before choosing the
display harness. Classify any new file per ADR 0007, run the headless and
desktop gates, update evidence docs, and stop before Slice 2.2F.
```
