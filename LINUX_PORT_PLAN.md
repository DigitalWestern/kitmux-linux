# Kitmux Linux Port Plan

**Status:** planning document for the Linux port only

**macOS reference:** `../macos/kitmux/`

**Reference commit:** `cc902462a97e35b955884b0f783820ef3d6ad6d4`

**Last reviewed:** 2026-07-23

**Current progress:** [`PORT_STATUS.md`](PORT_STATUS.md)

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

The reference checkout was dirty when this plan was reviewed. It contained an
unfinished `TerminalView` extraction and generated app bundles. The exact
dated observation is in [`PORT_STATUS.md`](PORT_STATUS.md). No agent should
discard those changes or freeze parity fixtures from that mixed state.

## What to inspect first

Before writing Linux code, a future agent should read these files in this order:

1. `PORT_STATUS.md`
2. `../macos/kitmux/AGENTS.md`
3. `../macos/kitmux/docs/AGENT_HANDOFF.md`
4. `../macos/kitmux/docs/IMPLEMENTATION_ROADMAP.md`
5. `../macos/kitmux/libkitty/README.md`
6. `../macos/kitmux/libkitty/include/libkitty.h`
7. `../macos/kitmux/libkitty/Makefile`
8. `../macos/kitmux/libkitty/src/`
9. `../macos/kitmux/libkitty/py/glue.py`
10. `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/main.swift`
11. `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/TerminalView.swift`
12. `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/PaneRuntime.swift`
13. `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/LibKitty.swift`
14. `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/ControlDispatcher.swift`
15. `../macos/kitmux/macos/KitmuxApp/Sources/Kitmux/SmokeSuite.swift`
16. `../macos/kitmux/macos/KitmuxApp/Tests/KitmuxCoreTests/`
17. `../macos/kitmux/macos/KitmuxApp/Makefile`

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
The production desktop host waits for the Phase 2 toolkit decision.

### Phase 0: Freeze the reference and contracts

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

#### Slice 0.2: Build the parity inventory

Create a machine-readable feature inventory. Every row needs:

- Stable ID and user-visible behavior.
- macOS source/test references.
- Classification: terminal alpha, beta, later, or intentionally omitted.
- Observable Linux acceptance statement.
- Dependencies and platform-translation notes.

Start with the ownership hierarchy, input/output, persistence, settings, close
safety, CLI, SSH, notifications, and browser surfaces.

#### Slice 0.3: Record decisions and support targets

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

#### Slice 0.4: Freeze portable fixtures

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

Exit criteria:

- The clean reference commit and tag are explicit.
- Every alpha feature has an observable acceptance statement.
- Every fixture has a version, bounds, malformed-input behavior, and macOS
  test.
- No fixture generation or import path executes a saved command.

### Phase 1: Prove headless `libkitty` on Linux

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
- Start the license inventory and SBOM.

Exit criteria:

- Ubuntu and Fedora containers build from clean checkouts and pass twice.
- Flood/close tests leave no live children or zombies.
- Moving the release tree to a new path does not break the tests.
- No developer path or undeclared native dependency remains.

Failure here blocks all product UI work.

### Phase 2: Run the rendering and toolkit kill spike

Goal: prove or reject GTK before product UI depends on it.

Build a disposable GTK 4 probe with:

- One `GtkGLArea`, one real libkitty session, and ordinary GTK controls beside
  it.
- Correct GL profile, framebuffer ownership, shader/font initialization, and
  GL state restoration.
- Resize, redraw, focus, clean close, and scale changes.
- Wayland and X11 runs.
- Key press/release/repeat, Compose, dead keys, AltGr, non-US layout, and a
  real IME preedit path.
- Selection, clipboard, paste, mouse reporting, wheel/touchpad, and search.
- PTY FD integration through GLib, with heartbeat/frame measurements during
  output flood.
- Fractional scaling and a move between unlike monitor scales.
- A minimal adjacent WebKitGTK widget only to expose focus, GL, dependency, and
  packaging conflicts.
- A visible diagnostic when GL initialization fails.

#### Slice 2.1: Host one real terminal surface

- Replace the clear-color smoke with a small GTK executable that owns one
  `GtkGLArea` and one real libkitty session.
- Initialize and tear down Kitty rendering only while a valid GTK GL context
  is current; preserve GTK's GL state around libkitty drawing.
- Integrate the PTY descriptor with the GLib main loop, queue redraws on
  output, propagate pixel/cell resize, and close/reap cleanly.
- Show an actionable in-window error when GL, Python, font, shader, or session
  initialization fails.
- Prove the slice visibly in the desktop VM over X11 before adding more UI.

#### Slice 2.2: Prove terminal interaction

- Add key press/release/repeat, Compose, dead-key, AltGr, non-US layout, and
  IME preedit/commit paths.
- Add focus, selection, clipboard, safe paste, mouse reporting, wheel, and
  search behavior.
- Keep ordinary GTK controls beside the terminal so focus transfer and
  shortcut routing are observable.

#### Slice 2.3: Make the toolkit decision

- Exercise Wayland and X11, scale changes, unlike monitor scales, and GL state
  restoration.
- Measure heartbeat/frame fairness during PTY flood and repeated resize.
- Add the minimal adjacent WebKitGTK conflict probe; it is not product browser
  work.
- Record GTK as chosen only if the complete decision gate below passes;
  otherwise run the one planned Qt 6 comparison.

Decision gate:

- Choose GTK only if GL correctness, input/IME, focus, scaling, Wayland/X11,
  and event fairness all pass on the support matrix.
- If a technical gate fails, run one equivalent time-boxed Qt 6 probe.
- If both fail, stop and redesign the renderer boundary.

Do not build production chrome around a failing spike.

### Phase 3: Build the Linux model and compatibility harness

Goal: reproduce portable product behavior without a display or real terminal.

#### Slice 3.1: Implement the pure model

- Stable workspace, group, tab, pane, surface, and split IDs.
- Split-tree layout and ratio constraints.
- Focus, navigation, reorder, and close-chain rules.
- Terminal/browser runtime interfaces backed by mocks.
- Hidden-session versus visible-layout ownership.

#### Slice 3.2: Implement bounded contracts

- State/settings decode and encode.
- Control framing, method IDs, errors, and size bounds.
- Command catalog IDs and semantic actions.
- Linux adapters for XDG paths, file watching, hashing, and socket addresses.

#### Slice 3.3: Prove compatibility and safe import

- Linux round-trips all portable golden fixtures.
- macOS consumes Linux-produced portable fixtures.
- A one-time macOS-state import preview reports accepted, translated, rejected,
  and inert command fields.
- Import validates paths and platform values without mutating the source.

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

#### Slice 4.2: Terminal interaction

- Kitty keyboard encoding and configurable app-shortcut routing.
- IME preedit/commit, selection, clipboard, scrollback, search, paste safety,
  mouse reporting, URL opening, and font controls.
- Resize/scale changes without session loss.
- Foreground-process close confirmation.

#### Slice 4.3: Minimal crash-safe persistence

- Atomic state/settings writes under XDG locations.
- Missing, corrupt, newer-schema, read-only, and full-disk behavior.
- File-watcher recovery after rename/replacement.
- Fresh shells on restore; never restore a live process or auto-run a command.

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

#### Slice 5.2: Splits and session ownership

- Nested splits and divider resizing.
- One permanent libkitty session per live terminal surface.
- Only the active surface receives ordinary input.
- Hidden terminals pump fairly but do not lay out or draw.

#### Slice 5.3: Product controls and persistence

- Command palette and settings UI.
- Hierarchy, ratios, titles, cwd metadata, safe settings, and surface selection
  persistence.
- Foreground-process review through the entire close chain.
- Keyboard-only navigation and initial AT-SPI roles/focus order.

Exit criteria:

- Cross-platform hierarchy fixtures stay green.
- Nested-layout, rapid-navigation, close-order, persistence, and hidden-output
  stress tests pass.
- Corrupt/interrupted state never silently replaces the last readable state.
- Core workflows are usable without a mouse.

### Phase 6: Add control, SSH, resume review, and reliability

#### Slice 6.1: Secure local control and CLI

- Private XDG runtime directory and `0600` socket.
- Owner/type/symlink checks and Linux peer credentials.
- Bounded frames, clients, reads, writes, timeouts, and event history.
- Explicit multiple-instance behavior.
- Package-managed CLI and a diagnosable user-local fallback.

#### Slice 6.2: SSH and agent workflows

- Resolve through the user's real `ssh` executable and `ssh -G`.
- Preserve reviewed argument arrays and explicit reconnect behavior.
- Handle nonstandard executable paths and missing desktop-session agent env.
- Keep secrets and expanded private arguments out of logs.
- Add hooks only after control identity is stable.

#### Slice 6.3: Resume and recovery

- Capture resume metadata as inert data.
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

This phase is optional for a terminal-only beta.

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

Packaging experiments start in Phase 1. Promotion order:

1. Reproducible CI development tarball.
2. Native `.deb` for the declared Ubuntu target.
3. Native RPM for the declared Fedora target.
4. AppImage only if glibc, GL, Python, and optional WebKit remain maintainable.
5. A separate Flatpak feasibility decision after host-shell semantics work.

Required evidence:

- Desktop entry, icons, AppStream metadata, and declared dependencies.
- Pinned builders and lockfiles.
- Checksums/signatures, SBOM, licenses, GPL source compliance, vulnerability
  results, and debug symbols.
- Fresh-VM install, desktop-menu launch, upgrade, downgrade, reinstall, and
  uninstall.
- No developer paths, keys, ambient Python, or unsigned mutable runtime code.

### Phase 9: Close parity and general availability

- Resolve every parity row as shipped, intentionally different, or deferred.
- Complete Tier 1 soak, accessibility, and threat-model reviews.
- Name maintainers for Kitty, Python, toolkit, optional WebKit, distro
  packages, and security updates.
- Publish release cadence, support intake, update SLA, and end-of-life policy.

Final gate:

- No open critical state-loss, command-execution, socket, rendering, input, or
  packaging defect.
- Published claims match the support matrix and distinguish Linux from macOS.

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

## Suggested repo shape

After Slice 0.3, create `kitmux-linux/` for contracts, fixtures, and proof
code. Only after the engine and rendering proofs pass should the wider checkout
evolve toward a shared-source shape like this:

```text
home-kitmux/
├── operating-system/
│   ├── macos/
│   │   └── kitmux/
│   └── linux/
│       ├── AGENTS.md
│       ├── LINUX_PORT_PLAN.md
│       ├── PORT_STATUS.md
│       ├── kitmux-linux/
│       └── support-matrix.yml
└── shared/
    ├── libkitty/
    └── contracts/
```

That shape is a goal, not a starting requirement.

`shared/` may instead become its own versioned repository or source package.
The invariant matters more than the folder name: macOS and Linux must consume
one authoritative libkitty and one authoritative contract-fixture set.

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

- Whether GTK 4 or Qt 6 wins the OpenGL and input-method spike.
- Whether libkitty becomes a separately versioned shared component.
- Which Linux distributions are Tier 1 versus source-only.
- How much of the macOS core should be shared by source versus by fixture.

When an open decision is resolved, record it in a short architecture decision
record and remove it from this list.

## Immediate next step

Use `PORT_STATUS.md` as the live checkpoint. At the 2026-07-23 handoff, the
clean macOS reference and initial Linux engine harness exist; Slice 1.2 is the
next implementation slice. Shared fixtures remain provisional and still block
the Rust product model.
