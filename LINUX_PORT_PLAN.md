# Kitmux Linux Port Plan

**Status:** active dependency plan; completion evidence lives in
[`PORT_STATUS.md`](PORT_STATUS.md)

**macOS reference:** `../macos/kitmux/`

**Reference commit:** `3088295003c0842d7c3198102d0d05378da4dc62`

**Last reviewed:** 2026-08-18 (Phase 5, v0.21 rebaseline, and Slices 6.1–6.3 complete; physical-Mesa and reproducibility gates next)

**Licence:** GPL-3.0-only. See [`LICENSE`](LICENSE) and ADR 0006.

**Current progress:** Phases 0 through 5 and Slices 6.1–6.3 are complete. GTK 4 is
the selected Linux UI toolkit. Phase 5's navigation hierarchy, split/session
ownership, product controls, full safe hierarchy persistence, close review, and
initial accessibility are complete on X11 and native Wayland, and the secure
local control socket, reviewed SSH workflows, and inert resume/recovery are
closed. See
[`PORT_STATUS.md`](PORT_STATUS.md) and [`NEXT_STEPS.md`](NEXT_STEPS.md).

**2026-07-25 audit changes:** Phase 2 was reordered so the checks that could
disqualify GTK run before the expensive ones that almost certainly cannot;
selection, clipboard, mouse, and search moved to Phase 4. Licensing became a
Phase 0 slice instead of a Phase 8 checklist item. Spike code gained explicit
durable-versus-disposable ownership. Reproducibility defects gained named
owners and due dates. Rationale is in ADRs 0006, 0007, and 0008 and in
[Why Slice 2.2 was cut short](#why-slice-22-was-cut-short).

## Purpose

This is a guide for a future coding agent that needs to build a native Linux
version of Kitmux without guessing at architecture, ownership, or order of
work.

The key decision is simple:

1. Keep macOS and Linux as separate hosts.
2. Prove the terminal engine on Linux first.
3. Treat macOS Swift/AppKit code as behavior reference, not as code to port
   line by line.
4. Build a Linux desktop host only after the engine and contracts are stable.

If a future agent reads only two sections, it should read
[What to inspect first](#what-to-inspect-first) and
[Implementation order](#implementation-order).

## What this plan is not

- Not a request to compile the AppKit app on Linux.
- Not a request to rewrite the macOS product before Linux exists.
- Not a claim that every macOS feature should ship in the first Linux release.
- Not a decision to share all source across platforms.

## Current reading of the codebase

The existing product has three different portability levels:

1. `libkitty` is the most reusable part. It already isolates the terminal
   engine behind a small C API, but its build and runtime helpers are still
   macOS-shaped.
2. `KitmuxCore` contains reusable behavior and schemas, but much of it is
   tied to Swift, Darwin, Combine, or macOS file-system assumptions.
3. The AppKit host is macOS-specific and should be treated as a live
   specification for Linux behavior, not as the Linux implementation plan.

The plan below turns that into work an agent can do in slices.

The live inspection also found an important split inside `libkitty` itself:

- The public C header and the C/Python session behavior are plausible shared
  engine code.
- The Makefile, portable-Python repair, render smoke, relocation audit,
  packaging scripts, and example host are currently Darwin-specific.

The reference checkout was dirty during the original plan review. That
`TerminalView` extraction was subsequently completed, tested, committed, and
tagged at `macos-linux-port-baseline-2026-07-23`. The exact evidence is in
[`PORT_STATUS.md`](PORT_STATUS.md). Future agents must still inspect the live
macOS status rather than assuming the dated checkpoint remains unchanged.

## What to inspect first

Before writing Linux code, a future agent should read these files in this order:

1. `PORT_STATUS.md`
2. `NEXT_STEPS.md`
3. `../macos/kitmux/AGENTS.md`
4. `../macos/kitmux/docs/AGENT_HANDOFF.md`
5. `../macos/kitmux/docs/IMPLEMENTATION_ROADMAP.md`
6. `../macos/kitmux/libkitty/README.md`
7. `../macos/kitmux/libkitty/include/libkitty.h`
8. `../macos/kitmux/libkitty/Makefile`
9. `../macos/kitmux/libkitty/src/`
10. `../macos/kitmux/libkitty/py/glue.py`
11. `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/main.swift`
12. `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/TerminalView.swift`
13. `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/PaneRuntime.swift`
14. `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/LibKitty.swift`
15. `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/ControlDispatcher.swift`
16. `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/SmokeSuite.swift`
17. `../macos/kitmux/macos/KitmuxApp/Tests/KitmuxCoreTests/`
18. `../macos/kitmux/macos/KitmuxApp/Makefile`

Those files tell the agent what must stay true even if the Linux stack is
implemented in a different language or toolkit.

## Implementation order

Work on one numbered slice at a time. A phase is complete only when its exit
gate has evidence in `PORT_STATUS.md`.

```text
Phase 0: clean reference and contracts
 ├── Phase 1: headless libkitty ── Phase 2: toolkit decision ──┐
 └── Phase 3: model and compatibility harness ────────────────┤
                                                              ▼
                                                  Phase 4: one terminal
                                                              │
                                                  Phase 5: multiplexer
                                                              │
                                                  Phase 6: reliability
                                                              │
                                     ┌────────────────────────┴────────┐
                                     ▼                                 ▼
                              Phase 7: browser                  Phase 8: packages
                                     └────────────────────────┬────────┘
                                                              ▼
                                                       Phase 9: parity/GA
```

After Phase 0 freezes the contracts, Phases 1 and 3 can proceed independently.
The Phase 2 decision selected GTK 4; production desktop work starts in Phase 4
after the closed Slice 0.6 reference-drift gate.

### Phase 0: Freeze the reference and contracts

Status: complete through Slice 0.6.

Goal: make sure Linux work is anchored to a clean macOS baseline.

#### Slice 0.1: Resolve the macOS worktree

- Read the live handoff and in-progress performance/refactor plan.
- Inspect the complete diff and all untracked source.
- Finish or intentionally abandon the current extraction without losing user
  work.
- Restore the required gates, commit the resolved work, and create a clearly
  named Linux-reference tag.

Minimum gate:

```sh
make -C libkitty test
make -C macos/KitmuxApp smoke
make -C macos/KitmuxApp persist-smoke
make -C macos/KitmuxApp browser-smoke
make -C macos/KitmuxApp control-smoke
make -C macos/KitmuxApp stress-smoke
make -C macos/KitmuxApp perf-smoke
git status --short --branch
```

Run any additional gates required by the live handoff and changed files. Do not
tag a failing or dirty baseline.

#### Slice 0.2: Build the parity inventory — complete

Create a machine-readable feature inventory. Every row needs:

- Stable ID and user-visible behavior.
- macOS source/test references.
- Classification: terminal alpha, beta, later, or intentionally omitted.
- Observable Linux acceptance statement.
- Dependencies and platform-translation notes.

Start with the ownership hierarchy, input/output, persistence, settings, close
safety, CLI, SSH, notifications, and browser surfaces.

#### Slice 0.3: Record decisions and support targets — complete

Create short decision records for:

- Rust as the initial language candidate.
- GTK spike criteria and the one-time Qt fallback.
- Linux repo location and the later shared-libkitty decision point.
- Contract versioning.
- Development Python versus bundled release Python.
- Package formats and update boundary.

Create `support-matrix.yml` with exact OS images, architecture, desktop,
display backend, GPU class, compiler, Python, toolkit, and package targets.
Changing versions belongs in that file rather than this plan.

Once the repository-location decision is recorded, create the Linux repository
with only the planning, contract, fixture, and spike structure needed for the
next slices. Do not scaffold production UI before the rendering gate.

#### Slice 0.4: Freeze portable fixtures — complete

Create valid and invalid fixtures for:

- State snapshots and stable IDs.
- Settings schema and defaults.
- Split-tree behavior and close order.
- Command identifiers.
- Control framing and limits.
- Portable SSH profile/review data.

Add macOS producer/consumer tests before calling a fixture authoritative.
Document nonportable paths, fonts, shortcuts, browser data, commands, and agent
sockets.

Closed on 2026-07-25 with `contracts/fixtures/v1/`: six contract files and 20
valid/invalid cases, a byte-identical macOS SwiftPM resource mirror, a
cross-repository drift validator, and macOS producer/consumer tests. Exact
commands and evidence are recorded in `PORT_STATUS.md`. Phase 3 must consume
the frozen cases; it does not regenerate them from Linux behavior.

#### Slice 0.5: Settle the licence posture — complete

Kitty is GPL-3.0-only, `libkitty` links it, and the host links `libkitty`.
Distributing a Linux artifact therefore places the whole combined work under
GPL-3.0. The only architecture that could have avoided this is an
out-of-process engine behind a defined protocol, which is a Phase 4 process-
model decision — so deferring the licence question to Phase 8 would have
silently foreclosed the option it was deferring.

Closed by ADR 0006: the Linux host is GPL-3.0-only free software, the
in-process architecture stands, and public source release becomes a
prerequisite for public binary distribution rather than a parallel track.

#### Slice 0.6: Define the re-baselining ritual — complete

The first macOS reference was frozen at
`macos-linux-port-baseline-2026-07-23`; the current reference is
`macos-linux-port-baseline-2026-08-02-v0.21`. macOS remains a live daily
driver, so the parity target keeps drifting and must remain measurable.

Produce a repeatable procedure that:

- reports the diff between the frozen tag and current macOS `HEAD`, restricted
  to `libkitty/`, `patches/`, and the files named in
  [What to treat as reusable](#what-to-treat-as-reusable);
- classifies each change as contract-affecting, behavior-affecting, or
  irrelevant to Linux;
- states when re-baselining is mandatory — a `libkitty` public header or
  patch change is mandatory, a macOS view change is not;
- re-locks `source-lock.json` and re-runs the headless gate as one reviewable
  commit that changes nothing else.

Run it at every phase boundary and record the result in `PORT_STATUS.md`, even
when the answer is "no relevant drift".

Closed on 2026-07-26 with
`kitmux-linux/scripts/report-reference-drift.py`. The default Markdown report
compares the locked tag to macOS `HEAD`, classifies every scoped file, lists
relevant uncommitted changes separately, and states whether rebaselining is
mandatory. `--patch` appends the restricted diff. The guarded
`--relock NEW_TAG` path requires clean macOS and Linux worktrees, requires the
tag to name current macOS `HEAD`, updates only `source-lock.json`,
materializes the new lock, and runs the Ubuntu headless gate before allowing
the one-file lock commit. Exact operating commands and the first drift result
are in `docs/LINUX_DEVELOPMENT.md` and `PORT_STATUS.md`.

Exit criteria:

- The clean reference commit and tag are explicit.
- Every alpha feature has an observable acceptance statement.
- Every fixture has a version, bounds, malformed-input behavior, and macOS
  test.
- No fixture generation or import path executes a saved command.
- The licence posture is recorded and the repository carries its licence text.
- Reference drift is measurable on demand.

### Phase 1: Prove headless `libkitty` on Linux

Status: complete through Slice 1.3, including attribution, SBOM, and repeatable
clean Ubuntu/Fedora ARM64 runtime gates.

Goal: produce a tested, relocatable `libkitty.so` without a GUI.

#### Slice 1.1: Create the Linux build harness

- Use the Linux repository location fixed in Slice 0.3.
- Consume tagged libkitty/Kitty source read-only or by hash-verified archive.
- Add CMake or Meson Linux targets without breaking the macOS Makefile.
- Compile the public header from C, C++, and Rust.
- Export only the intended C symbols.

#### Slice 1.2: Port the headless tests

Cover:

- Engine lifecycle, shell and argv launch, PTY resize, reads/writes, EOF, and
  child exit.
- Selection, scrollback, search, text extraction, paste, keyboard, mouse, OSC
  notifications/user variables, and cwd reporting.
- Repeated open/close, foreground-process detection, stopped/background jobs,
  and many-session floods.

#### Slice 1.3: Build the release-shaped runtime

- Separate a fast development runtime from the bundled release runtime.
- Package required Python, Kitty, config, font metadata, and native libraries.
- Use `$ORIGIN`-relative ELF rpaths.
- Audit exports, ELF dependencies, rpaths, and embedded paths.
- Prove the release layout does not import host Python accidentally.
- Generate and validate the license inventory and machine-readable SBOM.

Exit criteria:

- Ubuntu and Fedora containers build from clean checkouts and pass twice.
- Flood/close tests leave no live children or zombies.
- Moving the release tree to a new path does not break the tests.
- No developer path or undeclared native dependency remains.

Failure here blocks all product UI work.

### Phase 2: Run the rendering and toolkit kill spike

Goal: prove or reject GTK before product UI depends on it.

Status: complete. Slices 2.1 through 2.3 passed. Slices
2.2C–2.2F are the reordered kill tests; see
[Why Slice 2.2 was cut short](#why-slice-22-was-cut-short).

Build a GTK 4 probe with:

- One `GtkGLArea`, one real libkitty session, and ordinary GTK controls beside
  it.
- Correct GL profile, framebuffer ownership, shader/font initialization, and
  GL state restoration.
- Resize, redraw, focus, clean close, and scale changes.
- Wayland and X11 runs.
- Key press/release/repeat, Compose, dead keys, AltGr, non-US layout, and a
  real IME preedit path.
- PTY FD integration through GLib, with heartbeat/frame measurements during
  output flood.
- Fractional scaling and a move between unlike monitor scales.
- A minimal adjacent WebKitGTK widget only to expose focus, GL, dependency, and
  packaging conflicts.
- A visible diagnostic when GL initialization fails.

The probe is not uniformly disposable. Under ADR 0007 the display-free key
translation and its fixtures are durable and stay C behind the FFI boundary;
only `src/gtk_terminal_host.c` is replaced by the Phase 4 shell. Classify every
new file before writing it.

Selection, clipboard, paste safety, mouse, wheel, and search are deliberately
absent from this list. They are product behavior, they are not in doubt on
GTK, and they moved to Phase 4 Slice 4.2.

#### Slice 2.1: Host one real terminal surface — complete

- Replace the clear-color smoke with a small GTK executable that owns one
  `GtkGLArea` and one real libkitty session.
- Initialize and tear down Kitty rendering only while a valid GTK GL context
  is current; preserve GTK's GL state around libkitty drawing.
- Integrate the PTY descriptor with the GLib main loop, queue redraws on
  output, propagate pixel/cell resize, and close/reap cleanly.
- Show an actionable in-window error when GL, Python, font, shader, or session
  initialization fails.
- Prove the slice visibly in the desktop VM over X11 before adding more UI.

#### Slice 2.2: Prove terminal input — complete

- Add key press/release/repeat, Compose, dead-key, AltGr, non-US layout, and
  IME preedit/commit paths. Closed in Slices 2.2A and 2.2B.
- Keep ordinary GTK controls beside the terminal so focus transfer and
  shortcut routing are observable. Closed in Slice 2.2A.

Slice 2.2 originally also covered selection, clipboard, safe paste, mouse
reporting, wheel, and search. Those were reclassified on 2026-07-25 and moved
to Phase 4; see [Why Slice 2.2 was cut short](#why-slice-22-was-cut-short) and
ADR 0007.

#### Why Slice 2.2 was cut short

A kill spike exists to find the thing that kills the toolkit, as cheaply as
possible. Rank the remaining Phase 2 work by the chance GTK fails it against
the cost of finding out:

| Check | Could it kill GTK? | Cost |
| --- | --- | --- |
| WebKitGTK coexistence | plausibly | low |
| Native Wayland | plausibly | low |
| Fractional and mixed-monitor scaling | yes | medium |
| Main-loop fairness under PTY flood | maybe | low |
| Selection and clipboard | no — VTE ships it | high |
| Mouse, wheel, and search | no — VTE ships it | high |

Selection, clipboard, mouse, and search were the two most expensive remaining
items and the two least likely to fail. Every line of them is discarded if GTK
later fails a Wayland, scaling, or WebKit gate. Slices 2.2A and 2.2B already
answered the question those slices were nominally asking — GTK's input model
drives Kitty's keyboard protocol correctly, including a real input method — so
what remained was execution, not risk.

The dangerous checks now run first. Product interaction behavior moves to
Phase 4, where it is written once, in the chosen host language, against a
toolkit that has already survived.

#### Slice 2.2C: Prove Wayland — complete

- Run the existing host under a native Wayland compositor, not Xwayland.
- Prove `GtkGLArea` context creation, framebuffer ownership, and the tracked
  GL state restoration hold on the Wayland GL path.
- Prove key press/release/repeat and at least one input-method preedit/commit
  flow under Wayland, since key routing and IME differ from X11.
- Record what cannot be injected under Wayland: XTEST is X11-only, so name the
  substitute mechanism or mark the assertion display-bound.

Gate: the Slice 2.1 render, resize, GL-state, and clean-close proofs plus one
Slice 2.2A keyboard run, all green under native Wayland.

The 2026-07-25 gate passed under Weston 14.0.2 without Xwayland. GTK reported
`GdkWaylandDisplay`; the live libkitty recorder rendered and resized through
three exact framebuffers, restored tracked GL state, received native Wayland
press/release/repeat and IBus preedit/commit events, and was reaped on close.
Weston was nested visibly in the dedicated X11 VNC desktop. XTEST drove only
the compositor's outer window and Weston translated the input to
`wl_keyboard`, so this is explicitly display-bound and not physical-libinput
evidence. Exact results and limits are in `PORT_STATUS.md`.

#### Slice 2.2D: Prove the WebKitGTK conflict probe — complete

- Instantiate a minimal `WebKitWebView` adjacent to the live terminal in one
  process. This is a conflict probe, not browser product work.
- Look specifically for GL context conflicts, loader and symbol collisions
  against the isolated-`libpython` boundary in ADR 0002, focus interference,
  and the dependency closure a package would inherit.
- Run it under both display backends.

Gate: either a clean run under both backends, or a written, specific
incompatibility. An ambiguous result counts as a failure and needs a narrower
probe, not a workaround.

The 2026-07-25 gate passed with WebKitGTK 2.52.3 under X11 and native
Wayland. A static in-memory page and the live terminal rendered together;
tracked GL state, focus isolation and return, exact child bytes, clean child
reaping, and the isolated-libpython loader boundary all passed. The host's
native closure resolved 139 libraries with no missing objects or Kitty private
development-library leakage. This closes only coexistence risk, not browser
product behavior or packaging. Exact results and limits are in
`PORT_STATUS.md`.

This runs early on purpose. It is cheap, it is a known-hard interaction, and a
failure changes the toolkit decision — so discovering it after building product
chrome would be the single most expensive mistake available in this phase.

#### Slice 2.2E: Prove scaling — complete

- Prove coordinates, framebuffer size, cell metrics, and rendered text at
  100%, a fractional scale, and 200%.
- Move a live window between unlike monitor scales without session loss or
  cell-metric drift.
- Confirm `kitty_render_init`'s scale argument and
  `gtk_widget_get_scale_factor` stay consistent under fractional scaling, which
  is where GTK and Kitty are most likely to disagree.

Gate: exact framebuffer and cell assertions at each scale, and a scale change
applied to a live session.

Result: passed 2026-07-25. Nested Sway exposed 100%, 150%, and 200% virtual
outputs plus the fractional-scale and viewporter protocols. GTK reported
surface scales `1/1.5/2` and `GtkGLArea` backing factors `1/2/2`; Kitty rebuilt
its atlas for the backing factor. One child session moved across every output
and back to 100%, with exact framebuffer/cell/grid assertions and no PID,
content, or return-metric drift. This is virtual-output correctness evidence,
not physical mixed-DPI, GPU-performance, or packaging evidence.

#### Slice 2.2F: Prove event fairness

- Measure heartbeat and frame latency during sustained PTY flood, repeated
  resize, and multiple hidden sessions pumping at once.
- Confirm the GLib main loop does not starve UI events under output pressure,
  and that the existing `g_unix_fd_add_full` priority is defensible.

Gate: bounded, recorded latency figures under load. Absolute numbers on
llvmpipe are not performance evidence; the assertion is fairness, not speed.

Result: passed 2026-07-26 after the probe disproved the original source
priority. Five continuously readable sessions at `G_PRIORITY_DEFAULT`
allowed default-priority heartbeats but starved GTK's frame clock completely:
zero frames and resizes in 12 seconds. Moving only the PTY sources to
`G_PRIORITY_DEFAULT_IDLE` produced 550 heartbeats, 715 frames, 60 resizes, a
46.015 ms maximum heartbeat gap, a 24.779 ms maximum frame latency, and at
least 107 MB pumped per session in the same 12-second X11/llvmpipe run. This
is fairness evidence, not hardware performance evidence.

#### Slice 2.3: Make the toolkit decision

- Confirm Slices 2.1, 2.2A, 2.2B, and 2.2C through 2.2F are all green.
- Confirm the isolated-`libpython` boundary holds in a release-shaped GUI
  layout without a broad `LD_LIBRARY_PATH`.
- Confirm required accessibility focus and text-input behavior is viable.
- Record GTK as chosen only if the complete decision gate below passes;
  otherwise run the one planned Qt 6 comparison.

Decision gate:

- Choose GTK only if GL correctness, input/IME, focus, scaling, Wayland/X11,
  WebKit coexistence, and event fairness all pass on the support matrix.
- If a technical gate fails, run one equivalent time-boxed Qt 6 probe. Under
  ADR 0007 that probe replaces the disposable host file and re-targets the
  durable key translation's input struct; it does not restart key semantics.
- If both fail, stop and redesign the renderer boundary.

Do not build production chrome around a failing spike.

Result: GTK 4 selected 2026-07-26. Every Phase 2 runtime gate passed. A
temporary installed `bin/` + `lib/` GUI layout resolved `libkitty` and the
pinned `libpython` through relative runpaths without a broad
`LD_LIBRARY_PATH`, rendered a live frame, and reaped the child. The selected
terminal widget exposed GTK's terminal role and label, accepted focus,
transferred focus to a text control and back, and retained the proven
`GtkIMMulticontext` path. No decision criterion failed, so no Qt 6 probe was
warranted. This is source/GUI evidence in the ARM64 development VM, not native
package, clean-install, physical-GPU, or complete AT-SPI evidence.

At least one physical Mesa GPU must pass before beta. It is not a Phase 2
blocker: llvmpipe is adequate to prove correctness, and a GPU proves driver
behavior, which is a different question. Record it as a Phase 6 obligation
rather than stalling the toolkit decision on hardware access.

### Phase 3: Build the Linux model and compatibility harness

Goal: reproduce portable product behavior without a display or real terminal.

Status: complete through Slice 3.3.

#### Slice 3.1: Implement the pure model — complete

- Stable workspace, group, tab, pane, surface, and split IDs.
- Split-tree layout and ratio constraints.
- Focus, navigation, reorder, and close-chain rules.
- Terminal/browser runtime interfaces backed by mocks.
- Hidden-session versus visible-layout ownership.

The 2026-07-25 implementation lives in `kitmux-linux/rust/model`. Its host and
Ubuntu ARM64 gates format, lint with warnings denied, and run 15 headless tests,
including the frozen split-tree accept/reject cases. It has no GTK, WebKit,
libkitty, filesystem, shell, or network dependency. Exact results and limits
are recorded in `PORT_STATUS.md`.

#### Slice 3.2: Implement bounded contracts — complete

- State/settings decode and encode.
- Control framing, method IDs, errors, and size bounds.
- Command catalog IDs and semantic actions.
- Linux adapters for XDG paths, file watching, hashing, and socket addresses.

The 2026-07-25 implementation extends `kitmux-linux/rust/model` with bounded
state and settings codecs, the frozen command catalog, the 44-method control
surface and newline framing, XDG and Unix-socket address resolution, private
atomic writes, SHA-256 fingerprints, and replacement-aware polling. Host and
Ubuntu ARM64 gates pass 33 headless tests, including all applicable frozen
fixtures and invalid, oversized, newer-version, unknown-field, symlink, and
permission cases. The Ubuntu gate also passes with Cargo offline after its
locked dependencies are present. Exact commands, partial inventory limits,
and non-claims are recorded in `PORT_STATUS.md`.

#### Slice 3.3: Prove compatibility and safe import

- Linux round-trips all portable golden fixtures.
- macOS consumes Linux-produced portable fixtures.
- A one-time macOS-state import preview reports accepted, translated, rejected,
  and inert command fields.
- Import validates paths and platform values without mutating the source.

The 2026-07-26 implementation adds a bounded SSH profile/review consumer, a
temporary Linux-produced compatibility bundle consumed by the macOS
production codecs, and `kitmux-import-preview`. The preview reports accepted,
translated, rejected, and inert command fields; validates existing Linux
directories and safe `/Users/<name>` home translation; rejects unsafe values;
and has no source write, process, shell, socket, or network path. Host and
Ubuntu ARM64 model gates pass 37 tests, and the cross-host gate passes 9
macOS fixture tests. Exact commands and non-claims are in `PORT_STATUS.md`.

Exit criteria:

- Invalid, oversized, newer-version, and unknown-field inputs behave according
  to the contract.
- The suite needs no GPU, display, shell, WebKit, or network.
- Import never executes resume commands or shares one live state file.

### Phase 4: Build a one-terminal internal alpha

Goal: combine the engine, toolkit, and model in the smallest useful app.

#### Slice 4.1: Application shell and lifecycle

- One window, engine, terminal widget, and session.
- XDG config/state/cache/data/runtime paths.
- Account shell lookup and documented desktop-launch environment.
- PTY pumping, redraw scheduling, title/cwd updates, child exit, and ordered
  shutdown.
- Structured diagnostics that omit secrets.

Completed 2026-07-28. The release-shaped Rust/GTK app runs one passwd-shell
session, pumps and renders through the proven engine boundary, tracks
title/cwd and resize, and reaps the child. The app-only loader directory keeps
Kitty's bundled native closure isolated from distro GTK. The exact gate and
non-claims are recorded in `PORT_STATUS.md`.

#### Slice 4.2: Terminal interaction

This slice absorbed the work formerly planned as Slices 2.2C through 2.2E. It
is product behavior, written once in the chosen host language against a
toolkit that has already survived Phase 2.

Inherited from Phase 2 under ADR 0007, not rewritten:

- `src/gtk_key_translation.{c,h}`, called over FFI.
- The fixed-byte key matrix, the PTY recorder, and the XTEST injector.

New in this slice:

- Configurable app-shortcut routing that does not steal terminal Control keys.
  Slice 4.2 accepts Linux-only overrides for the seven implemented terminal
  actions; Slice 5.1 extends the same stable-command-ID path to navigation.
- Selection and text extraction through libkitty, with local selection
  distinguished from terminal mouse-reporting mode and a Shift override.
- Asynchronous clipboard copy and paste through the toolkit's own clipboard
  API, without blocking PTY pumping.
- Bracketed paste, plus confirmation for unsafe multiline or control-character
  paste per the macOS behavior contract.
- Mouse press/release/drag, wheel and touchpad scrolling, and URL hit-testing,
  converting toolkit coordinates to framebuffer pixels and terminal cells
  without assuming scale 1.
- Search query, next/previous result, marker visibility, and cancel.
- Scrollback, font controls, and URL opening.
- Resize and scale changes without session loss.
- Foreground-process close confirmation.

Gate: exact coordinate and cell assertions at each supported scale,
independent per-session selection and search state, a clipboard round trip,
rejected and confirmed unsafe-paste cases, and no lost PTY pumping during
sustained pointer activity.

Completed 2026-07-28. The production controller now owns the complete
one-terminal interaction path. Fresh X11 and native-Wayland release-runtime
gates pass; both supply external clipboard evidence and exercise input,
resize, search, font, mouse, wheel, and PTY progress during pointer activity.
X11 also supplies close-dialog evidence. Linux-only settings can override the
seven implemented actions by stable command ID; the configured search path is
proven on both backends. Navigation bindings remain assigned to Slice 5.1.
Exact evidence and limitations are in `PORT_STATUS.md`.

#### Slice 4.3: Minimal crash-safe persistence

- Atomic state/settings writes under XDG locations.
- Missing, corrupt, newer-schema, read-only, and full-disk behavior.
- File-watcher recovery after rename/replacement.
- Fresh shells on restore; never restore a live process or auto-run a command.

Completed 2026-07-28 by reusing the Phase 3 XDG/codecs/atomic-write/watcher
stack in the product app. The release-shaped persistence gate proves two-launch
safe restore, inert resume text, stable IDs, replacement recovery,
corrupt/newer/read-only behavior, and real `ENOSPC` preservation. Exact
evidence and limitations are in `PORT_STATUS.md`.

Exit criteria:

- Bash, zsh, fish, vim, less, and tmux exercise expected input paths.
- Wayland and X11 input/resize runs pass.
- A 30-minute flood/resize/interaction run stays responsive and leaves no
  child.
- A release-shaped build launches on a clean target without an SDK.

### Phase 5: Build the terminal multiplexer alpha

Goal: add Kitmux's core workspace value without browser dependencies.

#### Slice 5.1: Navigation hierarchy

- Workspaces, groups, terminal tabs, panes, surface containers, and stable IDs.
- Sidebar, tab strip, focus, naming, reorder, and close chain.
- Configurable Linux shortcuts that do not steal terminal Control keys.

Completed 2026-08-01: the existing display-free hierarchy owns bounded
workspace/group/tab naming, explicit non-empty tab/group/workspace closes, and
workspace cycling. Stable command IDs route navigation overrides, with
Super+1…9 and Alt+1…9 as separate workspace/tab namespaces that reject extra
modifiers. Host, Ubuntu-offline, cross-host fixture, inventory, and GTK app
Release-compile gates pass. A responsive GTK sidebar, group row, and tab row
route product create/select/rename/reorder/close behavior, and focused X11 plus
native-Wayland navigation gates pass from fresh release runtimes.

#### Slice 5.2: Splits and session ownership

- Nested splits and divider resizing.
- One permanent libkitty session per live terminal surface.
- Only the active surface receives ordinary input.
- Hidden terminals pump fairly but do not lay out or draw.

Completed 2026-08-01: the product registry owns one libkitty child and GLib PTY
source per stable terminal `SurfaceId`. A bounded C bridge renders the active
tab's nested leaves into scissored GL regions; inactive hierarchy remains
owned and pumps without entering layout or draw. Ratio-clamped pointer drag,
pointer/cycle/directional focus, and Super-based keyboard resize pass focused
X11 and native-Wayland split gates. Hidden-output variants prove continued PTY
draining after tab selection changes, active-only input routing, and complete
child reaping from fresh release runtimes.

#### Slice 5.3: Product controls and persistence

- Command palette and settings UI.
- Hierarchy, ratios, titles, cwd metadata, safe settings, and surface selection
  persistence.
- Foreground-process review through the entire close chain.
- Keyboard-only navigation and initial AT-SPI roles/focus order.

Completed 2026-08-01: a native GTK command palette and settings dialog reuse
the frozen catalog, bounded settings document, and shared command path. The
product projects and restores the complete terminal hierarchy, nested ratios,
schema-supported IDs, names/titles, surface selection, and safe per-surface
cwd into fresh shells without executing saved resume text. Pane, group,
workspace, and window closes share a scoped live foreground recheck. Focused
X11 and native-Wayland gates prove keyboard-only controls, native roles/focus,
full last-good recovery, close ordering, rapid navigation, hidden-session
pumping, child reaping, and the complete Phase 4 terminal regression.

Exit criteria:

- Cross-platform hierarchy fixtures stay green.
- Nested-layout, rapid-navigation, close-order, persistence, and hidden-output
  stress tests pass.
- Corrupt/interrupted state never silently replaces the last readable state.
- Core workflows are usable without a mouse.

### Phase 6: Add control, SSH, resume review, and reliability

Also due here, carried over from Phase 2: at least one physical Mesa GPU must
pass the rendering and interaction gates. llvmpipe proves correctness; a real
driver proves driver behavior, and that is a beta obligation rather than a
toolkit-decision blocker.

#### Slice 6.1: Secure local control and CLI

The Slice 6.1 audit is closed. The Ubuntu ARM64 headless
`kitmux-linux/scripts/test-model.sh` run passed the contract, model,
interaction, persistence, and 9-test `control_socket_tests.rs` suites,
including the bounded-client cap and dribbling-client total deadline. The
Ubuntu ARM64 X11 `DISPLAY=:1
kitmux-linux/scripts/test-phase6-control.sh` release-runtime gate passed the
private socket, owner/type/symlink checks, Linux peer credentials, bounded
frames, stale replacement, event history, basic and pane CLI dispatch,
multiple-instance ownership, default XDG resolution, user-local CLI
installation/diagnosis, and idle-client responsiveness.

- Private XDG runtime directory and `0600` socket.
- Owner/type/symlink checks and Linux peer credentials.
- Bounded frames and event history; client caps, I/O deadlines, and timeout
  behavior are proven by the headless Rust socket tests.
- Multiple instances are allowed: a second instance opens normally, logs
  `control_server_declined reason=live_server`, and runs without a control
  server while the first instance owns the socket; the X11 acceptance gate
  passed this case. Other socket-installation failures leave the terminal
  running with the reason shown in the status line.
- Package-managed CLI is present; the X11 acceptance gate passed the
  user-local fallback and its missing-source diagnostic.

#### Slice 6.2: SSH and agent workflows

- Resolve through the user's real `ssh` executable and `ssh -G`.
- Preserve reviewed argument arrays and explicit reconnect behavior.
- Handle nonstandard executable paths and missing desktop-session agent env.
- Keep secrets and expanded private arguments out of logs.
- Add hooks only after control identity is stable.

#### Slice 6.3: Resume and recovery

- Capture resume metadata as inert data. **Closed 2026-08-18.**
- Present every command unchecked for explicit review.
- Revalidate identity and unchanged text immediately before execution.
- Add packaged persistence, crash, socket, SSH, upgrade, and stress harnesses.
- Add bounded scrollback sidecars only if retained in scope.

Exit criteria:

- Socket attack and slow-client tests cannot block or crash the UI.
- No import/restore path executes a command automatically.
- Crash recovery does not overwrite newer or unreadable state.
- Packaged stress leaves no leaked processes, sockets, or mutable runtime code.

### Phase 7: Add browser and desktop integrations if approved

This phase is optional for a terminal-only beta and is deferred for the current
beta scope. Do not begin it without an explicit product-scope change.

- Add WebKitGTK only after the chosen toolkit and supported security branch are
  fixed.
- Define one owned browser data session and safe TLS, permission, popup,
  download, proxy, crash, and data-clearing behavior.
- Add terminal/browser surface stacks, focus isolation, resize, close, and
  restore.
- Add notifications through GIO/portal, focus-aware unread state, portal
  file/open-URL flows, drag/drop, and complete adjacent accessibility.

Exit criteria:

- Mixed and browser-only layouts pass on Wayland and X11.
- Web-process failure cannot crash or corrupt the workspace.
- Unsupported browser operations fail visibly and safely.

### Phase 8: Produce supported packages

Blocked until the reproducibility defects R1 and R2 in ADR 0008 are closed. A
package built from a tree nobody else can clone and build cannot satisfy the
source obligation ADR 0006 attaches to distributing it. This is a hard
dependency, not a quality preference.

Packaging experiments start in Phase 1. Promotion order:

1. Reproducible CI development tarball.
2. Native `.deb` for the declared Ubuntu target.
3. Native RPM for the declared Fedora target.
4. AppImage only if glibc, GL, Python, and optional WebKit remain maintainable.
5. A separate Flatpak feasibility decision after host-shell semantics work.

Required evidence:

- Desktop entry, icons, AppStream metadata, and declared dependencies.
- Pinned builders and lockfiles.
- Checksums/signatures, SBOM, licenses, vulnerability results, and debug
  symbols.
- GPL-3.0 corresponding source shipped or offered for the complete combined
  work — host, model, build scripts, and the exact Kitty and `libkitty`
  revisions used. Under ADR 0006 this verifies an obligation already
  satisfied; it is not an open question at this point.
- Fresh-VM install, desktop-menu launch, upgrade, downgrade, reinstall, and
  uninstall.
- No developer paths, keys, ambient Python, or unsigned mutable runtime code.

### Phase 9: Close parity and general availability

- Resolve every parity row as shipped, intentionally different, or deferred.
  This gate is only meaningful at the granularity the inventory is written at:
  a row covering an entire subsystem can be marked "shipped" while half the
  subsystem is missing. `contracts/feature-inventory.json` must be decomposed
  to per-behavior rows before Phase 5, not before Phase 9, because Phase 5 is
  where the hierarchy behaviors are actually built.
- Complete Tier 1 soak, accessibility, and threat-model reviews.
- Name maintainers for Kitty, Python, toolkit, optional WebKit, distro
  packages, and security updates. ADR 0008 R1 through R3 are what make this
  satisfiable; a project one person can build has no second maintainer.
- Publish release cadence, support intake, update SLA, and end-of-life policy.

Final gate:

- No open critical state-loss, command-execution, socket, rendering, input, or
  packaging defect.
- Published claims match the support matrix and distinguish Linux from macOS.
- Published claims state the GPL-3.0-only licence plainly.

## Linux rules the agent should not forget

### Files and state

- Use XDG paths, not macOS home-library paths.
- Handle missing or invalid `XDG_RUNTIME_DIR`.
- Use atomic write-and-rename where durability matters.
- Reject unsafe symlinks and wrong-owner files.

### Shell and PTY behavior

- Resolve the real login shell safely.
- Never execute shell strings when an argument vector exists.
- Handle PTY hangup, partial reads, signals, and process groups explicitly.

### Rendering and input

- Treat Wayland as the primary path and X11 as fallback.
- Preserve Kitty-style keyboard and mouse semantics.
- Handle IME, dead keys, AltGr, and non-US layouts.
- Do not make screenshot pixel equality the only render gate.

### Desktop integration

- Notifications should go through Linux desktop APIs or portals.
- Browser panes should be added only after the terminal workspace is stable.
- Accessibility should be designed into the widget hierarchy from the start.

### Control and security

- Use a private runtime socket path.
- Validate peer credentials.
- Bound message sizes and reject malformed input.
- Keep SSH launch behavior argument-vector based and secret-safe.

## What to treat as reusable

These files are useful as behavioral references or contract sources:

- `../macos/kitmux/libkitty/include/libkitty.h`
- `../macos/kitmux/libkitty/src/`
- `../macos/kitmux/libkitty/py/glue.py`
- `../macos/kitmux/libkitty/tests/`
- `../macos/kitmux/macos/KitmuxApp/Sources/KitmuxCore/StateSnapshot.swift`
- `../macos/kitmux/macos/KitmuxApp/Sources/KitmuxCore/SplitTree.swift`
- `../macos/kitmux/macos/KitmuxApp/Sources/KitmuxCore/ControlProtocol.swift`
- `../macos/kitmux/macos/KitmuxApp/Tests/KitmuxCoreTests/`
- `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/TerminalView.swift`
- `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/PaneRuntime.swift`
- `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/LibKitty.swift`
- `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/ControlDispatcher.swift`
- `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/SmokeSuite.swift`

These should not be copied blindly:

- `main.swift`
- AppKit view files
- macOS packaging scripts
- macOS-specific runtime repair scripts

## Repository direction

The current Linux and macOS repositories remain separate during the toolkit
spike. The recommended long-term destination is a private monorepo with
`platforms/macos`, `platforms/linux`, root `contracts`, and one authoritative
`engine/libkitty`. Preserve both histories and separate the history import
from the later shared-source extraction.

The complete proposal and migration verification checklist are in
[`docs/MONOREPO_MIGRATION.md`](docs/MONOREPO_MIGRATION.md). Do not perform the
migration during an implementation slice.

## Verification ladder

Use every level relevant to the slice and close every relevant level at the end
of a phase:

1. **Static:** formatting, lint, exported API, schema, dependencies, licenses.
2. **Unit:** pure model and bounded input behavior.
3. **Integration:** PTY, Python isolation, sockets, filesystem, and import.
4. **GUI:** toolkit, display backend, input/IME, scaling, and accessibility.
5. **Stress/failure:** floods, rapid lifecycle, resource exhaustion, GL loss,
   and corrupt/full/read-only storage.
6. **Artifact:** relocation, ELF/rpath, install, and package lifecycle.
7. **Clean system:** fresh VM or physical target without a developer SDK.

Record skipped levels explicitly. Containers prove builds and headless
behavior; they do not prove a compositor, GPU, desktop launcher, IME,
notification service, or accessibility stack.

## Stop rules

Stop the current lane and record the blocker when:

- The slice needs frozen behavior from a dirty or failing macOS reference.
- Linux appears to need a public C API change before an acceptance test proves
  why.
- The release runtime imports ambient Python or embeds developer paths.
- GTK fails a Phase 2 technical gate; use the planned Qt spike rather than
  accumulating production workarounds.
- Both toolkit spikes fail the same renderer boundary.
- State import, resume, CLI, or SSH work would execute unreviewed command text.
- A package needs broad sandbox escape or misleading distribution claims to
  appear functional.
- Completion needs credentials, paid infrastructure, signing identity, or
  destructive cleanup the user has not authorized.

## Practical guidance for future agents

- Read the live code before editing the plan.
- Work one dependency slice at a time.
- Keep the Linux plan honest about what is proven versus merely intended.
- Prefer explicit fixtures, tests, and ownership boundaries over long prose.
- Do not let the plan become a list of everything the product might someday do.

## Open decisions

These should be answered during implementation, not guessed in advance:

- Whether libkitty becomes a separately versioned shared component.
- Which Linux distributions are Tier 1 versus source-only.
- How much of the macOS core should be shared by source versus by fixture.
- Whether the macOS build carries the same GPL obligation as Linux. ADR 0006
  governs Linux only and is not a legal opinion; the owner should get one.
- When the repository becomes public. ADR 0006 makes it a prerequisite for
  binary distribution, not for today.

Resolved and removed from this list: the Linux licence posture (ADR 0006),
whether spike code carries forward (ADR 0007), and the GTK 4 toolkit decision.

When an open decision is resolved, record it in a short architecture decision
record and remove it from this list.

## Where effort should go next

In priority order, independent of which slice is nominally active:

1. **Slices 6.1 through 6.3 are closed**: secure local control, the CLI,
   reviewed SSH workflows, and inert resume/recovery have evidence in
   `PORT_STATUS.md`.
2. **Prove one physical Mesa GPU during Phase 6** before beta; do not treat
   llvmpipe correctness as driver evidence.
3. **Keep Phases 3 through 5 closed.** Cross-host compatibility, the read-only
   import preview, and the one-terminal alpha gates are proven; live import,
   browser, and packaging work stay in their assigned phases.
4. **Keep the feature inventory current** as Phase 6 adds product behavior.
5. **ADR 0008 R1, R2, and R5** — standalone buildability, one automated gate,
   and durable dependency mirrors — before Phase 8, and before any second
   contributor.

The recurring failure mode this plan should guard against is not lack of
rigor. Rigor here is high and per-slice evidence is excellent. It is spending
that rigor on code whose survival has not yet been established, while the
items that unblock everything else stay open.

## Immediate next step

Use `PORT_STATUS.md` as the evidence ledger and follow
[`NEXT_STEPS.md`](NEXT_STEPS.md). Phase 5, the macOS/libkitty v0.21 rebaseline,
and Slices 6.1–6.3 are complete; follow the physical-Mesa and reproducibility
order in `NEXT_STEPS.md`. Do not start browser product behavior.
Do not turn the closed Slice 3.3 preview into a live import or restore path
incidentally.
