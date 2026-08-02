# Kitmux Linux next steps

Phase 5, Slice 6.1 secure local control and CLI, and the mandatory clean
macOS/libkitty v0.21 rebaseline are closed. Begin **Slice 6.2 SSH and agent
workflows**. Slices 5.1 through 5.3 and Slice 6.1 pass
display-free, cross-host, focused X11, and native-Wayland
hierarchy, split, permanent-session, persistence, close-review, control,
accessibility, and rapid-navigation gates.

All Phase 2 kill tests passed, including event fairness, a temporary installed
GUI layout with relative loader paths, and GTK accessibility focus/text-input
viability. No GTK decision criterion failed, so the conditional Qt 6 probe was
not run. Phase 4 now covers selection, clipboard, mouse, wheel, search,
program input, persistence, sustained interaction, and clean-runtime launch.

Phase 0.4's portable fixture corpus and Phase 3 Slices 3.1 through 3.3 are
closed. The Rust model consumes every frozen contract family, owns the bounded
Linux adapters, exports temporary compatibility values consumed by macOS, and
provides a read-only state import preview. Do not expand the closed preview
into live import or restore work.

The clean tag `macos-linux-port-baseline-2026-08-02-v0.21` is locked and its
Ubuntu headless gate passes. Implement only Slice 6.2. Do not begin live macOS
import/restore, browser product functionality, packaging, or the monorepo
migration. Physical-Mesa GPU proof remains a Phase 6 beta obligation, not a
substitute for Slice 6.2's SSH/agent gate.

## Preconditions

1. Read `PORT_STATUS.md`, this file, the Phase 5 section of
   `LINUX_PORT_PLAN.md`, and ADRs 0006, 0007, and 0008.
2. Inspect `git status --short` and commits newer than the Slice 0.6 entry in
   `PORT_STATUS.md`; preserve any unrelated worktree changes.
3. Re-run the locked reference materialization check before product work:

   ```sh
   kitmux-linux/scripts/materialize-reference.sh
   ```

4. If the tagged baseline no longer materializes exactly, stop and record the
   drift before changing contracts or Linux behavior.

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

### 2.2E — Scaling — done 2026-07-25

- Nested Sway advertises the fractional-scale and viewporter protocols and
  supplies virtual outputs at 100%, 150%, and 200%.
- GTK's double surface scale maps logical coordinates to output pixels; its
  integer widget factor sizes the `GtkGLArea` backing buffer. The sequence is
  `1/1`, `1.5/2`, and `2/2`. Kitty rebuilds its font atlas for the integer
  backing factor and lets the compositor downsample at 150%.
- One live recorder session moved through all three outputs and back to 100%.
  Exact framebuffer, cell, logical/device cell, grid, content, and child-PID
  assertions passed; the original 100% metrics returned without drift.
- The narrow libkitty scale API is a hash-locked Linux overlay applied to the
  authoritative tagged source during materialization. It is display-free C
  behind the existing FFI boundary, not a second libkitty copy.
- Exact values, commands, proof images, and limitations are in
  `PORT_STATUS.md`.

## Completed final Phase 2 sequence

### 2.2F — Event fairness — done 2026-07-26

- The first five-session flood disproved `G_PRIORITY_DEFAULT`: heartbeats ran,
  but GTK delivered no frames or resizes.
- Moving only PTY sources to `G_PRIORITY_DEFAULT_IDLE` gave every session
  progress and bounded heartbeat, frame, and pump latency during 60 live
  resizes over 12 seconds.
- Exact figures and limits are recorded in `PORT_STATUS.md`.

This is X11/llvmpipe scheduler-fairness evidence, not hardware performance.

### 2.3 — Close Phase 2 and decide — done 2026-07-26

- Every Phase 2 gate passed in one final desktop run.
- A temporary installed `bin/` + `lib/` layout loaded `libkitty` and pinned
  `libpython` through relative runpaths without a broad `LD_LIBRARY_PATH`,
  rendered a live frame, and reaped the child.
- The terminal widget exposed GTK's terminal role and label, accepted focus,
  transferred focus to a `GtkEntry` and back, and retained the proven
  `GtkIMMulticontext` path.
- ADR 0002 records GTK 4 as selected. Full AT-SPI content/screen-reader,
  physical GPU, native package, and clean-install evidence remain later gates.

## Next dependency-safe sequence

### 0.6 — Make reference drift measurable — done 2026-07-26

- `kitmux-linux/scripts/report-reference-drift.py` compares the lock to macOS
  `HEAD`, classifies the scoped drift, separates uncommitted changes, and can
  append the restricted patch.
- The first report found only the intentional portable-fixture mirror and
  consumer-test commit. No public header, patch, engine, or behavior drift
  required a new baseline.
- `--relock NEW_TAG` guards the one-file lock update and runs the headless gate.
  Exact commands, evidence, and limits are in `PORT_STATUS.md`.

### 4.1 — Application shell and lifecycle — done 2026-07-28

The production Rust/GTK application owns one window, terminal surface, engine,
passwd shell, PTY source, render/resize/title/cwd lifecycle, and ordered child
shutdown. It builds without the disposable host or WebKitGTK/XTest and runs
from an isolated release tree whose app-only `lib/app` loader directory cannot
shadow distro GTK libraries.

Gate result: `test-phase4.sh` launched a fresh 894-file release runtime on X11,
proved the terminal child executable was the passwd shell, exercised title,
cwd, 50,000 output lines and repeated resizing, then exited and proved the
child was gone. Locked warnings-denied Clippy also passed. Exact evidence and
limitations are in `PORT_STATUS.md`.

### 4.2 — Terminal interaction — done 2026-07-28

Wire the existing durable key translator and pure interaction model into the
product app. Add shortcut-before-IME routing, asynchronous clipboard copy and
paste with unsafe-paste confirmation, local selection versus mouse-reporting
mode with Shift override, wheel/touchpad scroll, scale-aware cell mapping, URL
opening, search, font controls, and foreground-process close confirmation.

Gate: exact scale/cell assertions, independent libkitty session state,
clipboard round trips, rejected and confirmed unsafe paste, product behavior
on X11 and native Wayland, and continued PTY progress under pointer activity.

Both product gates pass from fresh release runtimes. The X11 run owns external
clipboard and close-dialog evidence; the nested native-Wayland run proves the
same production input/resize/search/font/mouse controller under
`GdkWaylandDisplay`. Exact evidence and non-claims are in `PORT_STATUS.md`.

### 4.3 — Minimal crash-safe persistence — done 2026-07-28

Reuse the existing XDG resolver, bounded state/settings codecs, private atomic
replace, and replacement-aware watcher. Persist only the one-terminal values
that exist now: safe cwd and font size. Missing files use defaults; malformed
or newer files are preserved and set aside; write failures keep the last
readable state; rename/replacement is detected. Restore always creates a fresh
passwd shell and never executes saved command text.

Gate: missing, corrupt, newer-schema, read-only, and full-disk behavior;
replacement watching; deterministic private writes; safe cwd fallback; and a
fresh shell with no restored command.

The display-free source gate and release-shaped X11 persistence gate pass. The
runtime proved two-launch cwd/font restore into a different fresh shell PID,
inert resume text, stable IDs, watcher recovery, corrupt/newer set-asides,
blocked/read-only preservation, and real `ENOSPC` on a private 64 KiB tmpfs.
Exact evidence and non-claims are in `PORT_STATUS.md`.

### Phase 4 exit matrix — done 2026-07-28

- Bash, Zsh, Fish, Vim, Less, and tmux input/return paths pass.
- Final X11 and native-Wayland product interaction/resize gates pass.
- The required 1,801-second soak passed 1,090 interaction cycles and 160 shell
  heartbeats with a 220 ms maximum heartbeat; shell and flood worker reaping
  passed.
- The release runtime launches under Xvfb in a runtime-only Ubuntu 26.04
  container with no SDK. Its base image is digest-pinned and fetchable when
  absent.

## Deferred but explicit

- Selection, clipboard, safe paste, mouse, wheel, and search closed in Phase 4
  Slice 4.2; do not reopen them during hierarchy work without a regression.
- Phase 0.4's shared valid/invalid fixture corpus is frozen. Phase 3 is closed
  through cross-host compatibility and the read-only import preview.
- Phase 0.6's macOS reference-drift report and guarded re-lock are complete.
  Run the report at every phase boundary and record even a no-drift result.
- `contracts/feature-inventory.json` now has per-behavior status and 97
  resolving Linux test references. Keep it current as Phase 6 adds behavior.
- ADR 0008 R1 and R2 remain: standalone buildability and one automated gate.
  R3 and R4 closed on 2026-07-28: the desktop gate is display-portable and
  restores touched session state, and both architecture bundles are locked.
- x86_64, GNOME, physical GPU, `.deb`/RPM, clean desktop install,
  upgrade/uninstall, and public-distribution work remain later gates.
- Repository migration is planned in `docs/MONOREPO_MIGRATION.md` and must be
  performed in a separate session.

## Completed Slice 5.1 checkpoint — 2026-08-01

- The existing pure hierarchy now owns bounded workspace/group/tab names and
  explicit tab/group/workspace close operations that preserve stable IDs,
  close removed runtimes, and refuse to empty their parent.
- Navigation settings now resolve stable command IDs through the same shortcut
  map as Phase 4 terminal actions. Super+1…9 addresses workspaces and Alt+1…9
  addresses terminal tabs without taking plain Control chords from terminal
  applications.
- `test-phase3.sh` passed 50 Rust tests and all 9 macOS portable-contract
  consumers. The same 50 tests passed offline in Ubuntu ARM64, and a fresh
  temporary CMake Release build compiled `kitmux_app` against GTK 4.22.4.
- The responsive GTK sidebar, group row, and tab row now route create, select,
  rename, reorder, close, and stable-command shortcuts. Fresh release-runtime
  navigation gates and the full Phase 4 terminal regression pass on X11 and
  native Wayland. Slice 5.1 is closed.

## Completed Slice 5.2 checkpoint — 2026-08-01

- Every live terminal surface owns a permanent libkitty child and idle-priority
  GLib PTY source keyed by stable `SurfaceId`; removed surfaces and window
  teardown close and reap every owned child.
- One GTK GL area renders the active tab's nested split leaves through a
  bounded scissored multi-region bridge. Pointer focus, tolerant divider drag,
  cycle/directional focus, and Super-based keyboard resize route through the
  existing pure model and ratio bounds.
- Inactive tabs/workspaces remain owned and drain output without entering the
  visible layout/draw set. Ordinary input always routes through the selected
  surface, including key-up after navigation.
- The 51-test Ubuntu model gate, X11 and native-Wayland nested split gates, and
  X11 and native-Wayland hidden-output gates pass from fresh 894-file release
  runtimes. Slice 5.2 is closed.

## Completed Slice 5.3 and Phase 5 checkpoint — 2026-08-01

- The native GTK command palette filters the frozen catalog deterministically
  and routes supported actions through the shared product path. The native
  settings dialog edits bounded settings, preserves unknown fields, applies
  live, and is fully usable by keyboard.
- State now round-trips the complete terminal hierarchy, nested ratios,
  schema-supported IDs, names/titles, selections, surface stacks, and safe
  per-surface cwd into fresh passwd shells. Saved commands stay inert and a
  corrupt primary recovers the byte-identical last-good hierarchy.
- Pane, group, workspace, and window closes use one scoped foreground recheck.
  Native roles/labels and terminal → Commands → Settings → terminal focus
  transfer pass on X11 and native Wayland.
- The 52-test Linux source gate and all 9 macOS portable consumers pass. Fresh
  894-file release gates pass the complete product scenario, X11/native-
  Wayland rapid navigation, nested split/accessibility, hidden-output, and the
  full Phase 4 X11/native-Wayland regressions.
- Phase-boundary drift was mandatory: clean macOS `HEAD` added the v0.21
  multi-context render-resource release API. The clean tag and guarded Ubuntu
  headless re-lock passed on 2026-08-02; this is closed Phase 0.6 maintenance,
  not unfinished Slice 5 work.

## Completed Slice 6.1 checkpoint — 2026-08-02

- The private local control socket and `kitmuxctl` now cover bounded framing,
  peer credentials, stale replacement, event history, hierarchy/pane dispatch,
  and release-runtime installation.
- The Ubuntu ARM64 X11 control gate passes slow-client, malformed/oversized,
  symlink, owner/mode/type, stale-restart, and CLI checks. SSH, resume, and
  physical-GPU evidence remain later Phase 6 work.

## Resume prompt

```text
Read AGENTS.md, PORT_STATUS.md, NEXT_STEPS.md, Phases 5 and 6 in
LINUX_PORT_PLAN.md, and ADRs 0007 and 0008. Phases 0 through 5 are closed; GTK 4
is selected, and the full terminal multiplexer alpha passes source,
cross-host, X11, and native-Wayland gates. Preserve unrelated worktree changes.
The clean macOS/libkitty v0.21 reference is locked and its headless gate
passes. Implement only Slice 6.2 SSH and agent workflows. Do not begin
live macOS import/restore, browser product work, packaging, or repository
migration; keep physical-GPU proof explicit as the separate Phase 6 beta gate.
```
