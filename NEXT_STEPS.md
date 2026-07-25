# Kitmux Linux next steps

Start with Slice 2.2C. Slices 2.2A and 2.2B are closed; do not begin product
navigation UI, the Rust product model, browser functionality, packaging, or
the monorepo migration.

## Preconditions

1. Read `PORT_STATUS.md`, this file, and the Slice 2.2/2.3 sections of
   `LINUX_PORT_PLAN.md`.
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

## Slice 2.2 implementation sequence

Keep each step independently reviewable and keep a normal GTK control beside
the terminal throughout the spike.

### 2.2A — Deterministic keyboard harness — done 2026-07-25

Exact commands, byte expectations, and limits are in `PORT_STATUS.md`. What
later sub-slices inherit:

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
  `PORT_STATUS.md` and in the tool's own header comment.
- The host reports every translated event with its exact bytes, plus focus
  ownership and widget bounds, on stdout. Keep that contract: the desktop gate
  asserts against it.

### 2.2B — Compose, layouts, and IME — done 2026-07-25

Exact commands, byte expectations, and limits are in `PORT_STATUS.md`. What
later sub-slices inherit:

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
  on exit.

Backend-independent versus desktop-only: the encoder expectations for
layouts and AltGr live in `tests/gtk_key_matrix.c` and need no display. Every
Compose, dead-key, preedit, and commit assertion needs the desktop session,
because only a live `GtkIMContext` produces them.

### 2.2C — Focus, selection, clipboard, and safe paste — next

- Make terminal/GTK-control focus transitions explicit and observable.
- Wire selection and text extraction through libkitty.
- Implement asynchronous GDK clipboard copy/paste.
- Preserve bracketed-paste semantics and require confirmation for unsafe
  multiline/control-character paste according to the macOS behavior contract.

Gate: automated focus ownership, copy round-trip, ordinary paste,
bracketed-paste, and rejected/confirmed unsafe-paste cases.

### 2.2D — Mouse, wheel, and search

- Convert GTK coordinates to framebuffer pixels and terminal cells without
  assuming scale 1.
- Cover local selection versus terminal mouse-reporting mode, press/release,
  drag, hover where supported, Shift override, wheel/touchpad scrolling, and
  URL hit testing.
- Wire search query, next/previous result, marker visibility, and cancel.

Gate: exact coordinate/cell assertions at the current X11 scale, independent
selection/search state, and no lost PTY pumping during pointer activity.

### 2.2E — Close the interaction slice

- Run the full headless and desktop gates from a clean build directory.
- Capture one visible proof that includes terminal input, IME/preedit, focus
  transfer, selection/search, and the adjacent GTK control.
- Check child reaping, FD restoration, warnings-as-errors, and the loader
  report.
- Update `contracts/feature-inventory.json`, `support-matrix.yml`, ADR 0002,
  and `PORT_STATUS.md` with exact commands/results.

Exit gate: all Slice 2.2 interaction behaviors pass over X11 in the desktop
VM. Anything not automated must remain explicitly unproven.

## Slice 2.3 evidence required before selecting GTK

Do not write a permanent GTK selection ADR until all of these have evidence:

1. The Slice 2.1 rendering/lifecycle and Slice 2.2 interaction gates stay
   green.
2. The same terminal/input path passes under native Wayland and X11.
3. Coordinates, framebuffer size, text, and pointer behavior pass at 100%,
   fractional scale, 200%, and when moving between unlike monitor scales.
4. At least one physical Mesa GPU passes; llvmpipe is not performance or
   driver evidence.
5. Heartbeat/frame latency remains bounded during sustained PTY flood,
   repeated resize, and multiple hidden sessions.
6. A minimal adjacent WebKitGTK widget exposes no unacceptable GL context,
   focus, loader, dependency, or packaging conflict. It is a conflict probe,
   not browser product work.
7. The isolated-libpython boundary works in a release-shaped GUI layout
   without broad `LD_LIBRARY_PATH`.
8. Required accessibility focus and text-input behavior is viable.

If a criterion fails for a concrete GTK limitation, run one time-boxed,
equivalent Qt 6 probe. If both toolkits fail the same renderer boundary, stop
and redesign that boundary rather than accumulating workarounds.

## Deferred but explicit

- Phase 0.4 shared valid/invalid fixtures still blocks the Rust product model.
- x86_64 Ubuntu/Fedora, GNOME, physical GPU, `.deb`/RPM, clean desktop
  install, upgrade/uninstall, and public-distribution work remain later gates.
- Wayland, scaling, fairness, and WebKit coexistence belong to Slice 2.3, not
  an inflated Slice 2.2.
- Repository migration is planned in `docs/MONOREPO_MIGRATION.md` and must be
  performed in a separate session.

## Resume prompt

```text
Read AGENTS.md, PORT_STATUS.md, NEXT_STEPS.md, and the Slice 2.2 section of
LINUX_PORT_PLAN.md. Confirm the worktree and both VM baselines. Implement only
Slice 2.2C: make terminal/GTK focus transitions explicit and observable, wire
selection and text extraction through libkitty, implement asynchronous GDK
clipboard copy and paste, and preserve bracketed-paste semantics plus the
macOS confirmation contract for unsafe multiline or control-character paste.
Extend the existing harness rather than replacing it. Run the headless and
desktop gates, update evidence docs, and stop before Slice 2.2D.
```
