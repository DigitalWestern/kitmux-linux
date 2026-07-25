# Kitmux Linux next steps

Start with Slice 2.2C — **Wayland**. Slices 2.2A and 2.2B are closed.

Phase 2 was reordered on 2026-07-25. Slice 2.2C is no longer selection and
clipboard: those, along with mouse, wheel, and search, moved to Phase 4 as
product work. The remaining Phase 2 slices are the checks that could actually
disqualify GTK, cheapest and most dangerous first. The reasoning is in
[Why Slice 2.2 was cut short](LINUX_PORT_PLAN.md#why-slice-22-was-cut-short)
and ADR 0007.

Do not begin product navigation UI, the Rust product model, browser
functionality, packaging, or the monorepo migration.

## Preconditions

1. Read `PORT_STATUS.md`, this file, the Phase 2 section of
   `LINUX_PORT_PLAN.md`, and ADRs 0006, 0007, and 0008.
2. Confirm `git status --short` is clean and inspect commits newer than the
   Slice 2.2B entry in `PORT_STATUS.md`.
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
  `PORT_STATUS.md` and in the tool's own header comment. It is X11-only, which
  Slice 2.2C has to work around.
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

## Remaining Phase 2 sequence

### 2.2C — Wayland — next

The existing host, unchanged in behavior, running under a native Wayland
compositor rather than Xwayland.

- Prove `GtkGLArea` context creation, framebuffer ownership, and the tracked
  GL state restoration on the Wayland GL path. GTK's GL backend differs enough
  from X11 that Slice 2.1's evidence does not transfer.
- Prove key press/release/repeat and at least one input-method preedit/commit
  flow. Key routing and IME differ between the backends; this is not a rerun.
- `tests/x11_key_injector.c` is XTEST and will not work here. Either find a
  substitute injection mechanism — a virtual input device, or the compositor's
  own testing interface — or mark the affected assertions display-bound and
  say so explicitly rather than quietly dropping them.
- Weston is already installed in the desktop VM. Prefer it over adding a new
  environment.

Gate: the Slice 2.1 render, resize, GL-state, and clean-close proofs plus one
Slice 2.2A keyboard run, all green under native Wayland.

If GTK's Wayland GL path cannot host Kitty's renderer, stop and record it.
That is a toolkit-disqualifying result and it is exactly what this slice is
for.

### 2.2D — WebKitGTK conflict probe

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

Gate: a clean run under both backends, or a written, specific
incompatibility. An ambiguous result is a failure — narrow the probe, do not
work around it.

This is deliberately early. It is cheap, it is a known-hard interaction, and a
failure changes the toolkit decision. Finding it after Phase 4 exists would be
the most expensive mistake available in this phase.

### 2.2E — Scaling

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
- Phase 0.4 shared valid/invalid fixtures still block the Rust product model,
  and through it Phases 4 and 5 — the entire shippable alpha. This is the
  critical path and it has been open since the beginning.
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
reordered: Slice 2.2C is Wayland, not selection/clipboard. Confirm the
worktree and both VM baselines. Implement only Slice 2.2C: run the existing
GTK host under a native Wayland compositor and prove GtkGLArea context
creation, framebuffer ownership, tracked GL state restoration, resize, clean
close, key press/release/repeat, and one input-method preedit/commit flow.
XTEST will not work there — either substitute an injection mechanism or mark
the affected assertions display-bound explicitly. Extend the existing harness
rather than replacing it, classify any new file as durable or disposable per
ADR 0007, run the headless and desktop gates, update evidence docs, and stop
before Slice 2.2D.
```
