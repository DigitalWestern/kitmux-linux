# Kitmux Linux Port Status

**Last inspected:** 2026-07-25

**Implementation state:** Phase 1, GTK Slice 2.1, and Slice 2.2A are closed.
Separate Ubuntu ARM64 headless and XFCE desktop VMs, an ELF `libkitty.so`, a
relocatable 104 MB attributed engine runtime, Linux stress tests, a real
one-session GTK 4 terminal host, and a deterministic keyboard-input harness
exist. No product navigation UI or native package exists yet.

**Active phase:** Phase 2 — rendering and toolkit kill spike.

**Next slice:** Slice 2.2B — Compose, dead keys, AltGr, a non-US layout, and a
real IBus preedit/commit flow through `GtkIMContext`. Slice 2.2A proved
physical key press/release/repeat routing and focus transfer only; the host
still synthesizes committed text from key symbols, which 2.2B must replace.
Phase 0.4 shared fixture promotion remains open and blocks the Rust product
model, not this GTK spike.
The concrete sub-slice order is in [`NEXT_STEPS.md`](NEXT_STEPS.md).
Operational VM, gate, runtime, and SBOM commands are in
[`docs/LINUX_DEVELOPMENT.md`](docs/LINUX_DEVELOPMENT.md).

## Verified checkout state

Adjacent macOS checkout: `../macos/kitmux/`

Clean reference revision:
`e39381a0ed6c3d1667cb4dfa70e5bc48213b1bc4`
(`style: normalize extracted source endings`)

Reference tag:
`macos-linux-port-baseline-2026-07-23`

Pinned Kitty revision inspected:
`c1d507dbe8cd12830d8b97b0d350d9dc2e4d383f`

The macOS source worktree was clean when tagged. Generated `Kitmux 3.app`,
`Kitmux 4.app`, and `Kitmux 5.app` bundles remain preserved on disk and are
listed only in the checkout's local Git exclude file.

## Planning findings

- `libkitty` exposes a useful C API, but its Makefile, Python-runtime repair,
  render smoke, example host, relocation audit, and packaging are currently
  macOS-specific.
- The C wrapper and Python glue are plausible shared engine code, subject to
  Linux build, PTY, ELF, GL, and bundled-runtime proof.
- The Swift core is most useful first as behavior and fixtures. Some files
  directly depend on Darwin, Combine, CryptoKit, Foundation behavior, or
  macOS paths.
- GTK 4 is the first toolkit candidate, not a final choice. The disposable
  host is C to test the direct libkitty/GTK boundary; Rust remains the future
  model/lifecycle candidate after shared fixtures are authoritative. Input,
  scale, display-backend, and event-loop evidence decides the production
  stack.
- The first user-facing target is a terminal-only alpha. Browser panes and
  broad distribution support are later decisions.

## Evidence log

### 2026-07-25 — Slice 2.2A deterministic keyboard input and focus

- Added `src/gtk_key_translation.{c,h}`: a display-free translation from GDK
  key vocabulary to the public `kitty_key_event` contract (functional-key
  numbering, kitty's GLFW-fork modifier bits, base-layout key identity with
  the shifted codepoint alongside, and kitty's text-suppression rules), plus
  a held-key tracker that separates press from auto-repeat. Encoding itself
  stays in kitty through `kitty_session_encode_key`, so DECCKM and the
  keyboard-protocol flag stack come from the live session.
- The GTK host now routes focused key events through that path, keeps an
  ordinary `GtkEntry` beside the terminal, reports focus ownership, widget
  bounds, and every translated event with its exact encoded bytes, can run a
  fixture child instead of a login shell, and can close on child exit.
- Added `tests/pty_input_recorder.c`, a raw-mode PTY child that records the
  exact bytes it receives, can emit a fixed escape sequence at startup to put
  the terminal in a known state, and echoes each read back as hex so the
  rendered screen shows what arrived.
- Added `tests/gtk_key_matrix.c`: 30 key events × 3 live terminal states, with
  expectations written from kitty's documented protocol and its pinned
  `key_encoding.c` rather than captured from this host. It asserts the
  translated event metadata, the exact encoded bytes, and the bytes the real
  PTY child read: 71 bytes in the default and DECCKM states, 186 with kitty
  keyboard-protocol flags 15.
- Added `tests/x11_key_injector.c` (XTEST) and `libxtst-dev` to desktop
  provisioning. `xdotool` is unusable for the function keys here: it infers
  modifiers from the shift level where it finds a keysym, and this session's
  XKB map places `XF86Switch_VT_*` on higher levels of the same keycode, so
  `xdotool key F1` injects keycode 64 (Alt_L) before keycode 67 and XFCE
  consumes the result as Alt+F1. `xinput test-xi2 --root` confirmed the
  injected keycodes; flattening the keymap with `xmodmap` did not fix it.
- GUI-tested on GTK 4.22.4, X11, Mesa llvmpipe, via two windowed runs whose
  child byte streams are matched against fixed expectations:
  - default terminal state — `a`, Shift+`b`, Ctrl+`c`, Alt+`d`, Enter, Tab,
    Backspace, Escape, four arrows, and F1 produced
    `61 42 03 1b64 0d 09 7f 1b 1b5b41 1b5b42 1b5b44 1b5b43 1b4f50`; releases
    correctly produced no bytes; a held `a` produced one press and nine real
    GDK auto-repeats, each encoding to `61`.
  - kitty keyboard protocol (`CSI > 15 u`, set by the child at startup) —
    press, repeat, and release each carried distinct bytes end to end, for
    example `a` as `1b5b393775`, `1b5b39373b313a3275`, and
    `1b5b39373b313a3375`.
- Focus transfer is automated: clicking the adjacent `GtkEntry` moves focus,
  the entry receives `xy`, the terminal child receives nothing while it is
  unfocused, clicking back returns focus, and the next key (`z` → `7a`)
  reaches the terminal again. Both runs ended by the fixture exiting on a
  sentinel, and both left their recorded child PID dead.
- Visible evidence is `kitmux-linux/gtk-keyboard-focus-proof.png`: the
  terminal rendering the exact received bytes, the entry holding `xy`, and the
  ordinary GTK controls.
- Source-tested: `kitmux-linux/scripts/test-headless.sh` — six tests passed in
  6.32 seconds plus the Rust/C header layout check.
- GUI-tested: `kitmux-linux/scripts/test-desktop.sh` — the Slice 2.1 render,
  resize, PTY, GL-state, and clean-close proofs stayed green alongside the new
  keyboard matrix and both windowed key runs.
- Not proven by this slice: Compose, dead keys, AltGr, non-US layouts, IME
  preedit/commit, clipboard, paste, mouse, wheel, search, Wayland, scaling,
  and physical GPUs. The host still synthesizes committed text from key
  symbols; Slice 2.2B must replace that with `GtkIMContext`.
- Package-tested: none.

### 2026-07-24 — durable Slice 2.1 handoff

- Added a GitHub-facing project README, a single Linux operations/evidence
  guide, an exact Slice 2.2 sub-slice handoff, and a history-preserving
  monorepo proposal. No migration or GTK functionality was performed.
- Added a pinned headless Lima definition alongside the existing desktop
  definition so both VMs can be recreated without developer-local state.
- Reconciled the plan, feature inventory, support matrix, and ADRs with the
  actual C GTK spike, the pending GTK decision, and the isolated-libpython
  boundary.
- The documentation audit found an ignored 298 MiB local runtime from an older
  builder. It was preserved, not treated as evidence. Fresh build paths and
  the verified approximately 104 MiB clean-gate result are now distinguished
  explicitly.
- Documentation verification passed `git diff --check`, JSON parsing, YAML
  parsing, both `limactl validate` checks, and the local Markdown-link audit.

### 2026-07-24 — GTK Slice 2.1 real terminal surface

- Replaced the clear-color `GtkGLArea` smoke with `kitmux_gtk_host`: one real
  libkitty engine/session, host-owned GTK framebuffer, GLib PTY source, damage
  redraws, title/bell/exit callbacks, framebuffer resize, a normal GTK status
  row and Close control, and an in-window diagnostic overlay.
- The first link exposed a real toolkit collision: libkitty's broad Python
  development runpath shadowed GTK's distro Cairo, HarfBuzz, GLib, and
  xkbcommon. The host now links only an isolated copy of the pinned
  `libpython`; Kitty's full development-library directory never enters the GTK
  process loader path. This boundary is recorded in ADR 0002.
- `kitmux-linux/scripts/test-desktop.sh` built with warnings-as-errors and
  passed on GTK 4.22.4, X11, and Mesa llvmpipe. The live shell pumped 299 PTY
  bytes through GLib, rendered at 900x530, resized to 760x450 and 980x590,
  restored the tracked host GL state after Kitty draw, displayed the missing
  runtime diagnostic in a separate run, and left its recorded child PID dead
  after the automated close.
- Visual evidence is `kitmux-linux/gtk-terminal-host-proof.png`; it shows the
  GTK control row and Kitty-rendered shell prompt/cursor at the final size.
- `kitmux-linux/scripts/test-headless.sh` remained green: six tests passed in
  6.32 seconds plus the Rust/C header layout check.
- GUI-tested: X11 plus software OpenGL only. Wayland, real GPU, input/IME,
  pointer/clipboard, fractional scaling, flood fairness, and adjacent WebKit
  remain required before choosing GTK.
- Package-tested: none. The isolated-libpython development layout is an
  architecture finding, not package evidence.

### 2026-07-24 — Slice 1.3 release runtime closed

- Replaced the copy-every-library release layout with an ELF-derived recursive
  closure: 25 bundled SONAMEs and 10 declared system SONAMEs. Removed Kitty C
  build sources, caches, developer launcher artifacts, Python build config,
  and unused build-time Python packages. The runtime fell from roughly 303 MB
  to 104 MB without changing the stress result.
- Added an authoritative 24-component runtime manifest, 23 per-library
  upstream license notices reconstructed from SHA-256-verified source
  archives, a deterministic SPDX 2.3 JSON SBOM covering 876 regular files,
  and per-file SHA-1/SHA-256 plus package verification codes. The generated
  SBOM passed the official `spdx-tools` full-document validator.
- Hardened the release audit to reject unowned files, components without
  payloads, missing notices, stale SBOM and payload checksums, undeclared ELF
  dependencies, host Python resolution, broken relocatability, and generic
  developer checkout paths. Locked the ARM64 dependency and Nerd Font bundle
  hashes and cross-checked 21 component source hashes against Kitty's pinned
  source inventory.
- Removed hard-coded Python-directory lookup and made Kitty platform selection
  architecture-aware. Stopped mutating the cached dependency interpreter,
  fixed non-canonical compiler prefix mapping, normalized shipped ELF modes,
  replaced a one-file fontconfig repair with full locked-archive recovery, and
  fixed the clean gate so it tests the candidate worktree instead of `HEAD`.
- Source-tested: `kitmux-linux/scripts/test-headless.sh` passed all six
  C/C++/ELF/engine/session/stress tests in 6.34 seconds plus the Rust/C header
  layout check.
- Clean-runtime-tested: `kitmux-linux/scripts/test-clean-containers.sh` passed
  two isolated Ubuntu 26.04 ARM64 builds and two isolated Fedora 44 ARM64
  builds. Every build passed from a random staging path and after relocation,
  including 16-session flood, 24 forced-close/reap cycles, zombie absence, and
  exact FD restoration. Repeated inventories were byte-identical within each
  distribution: Ubuntu
  `c3c0e71fc8dce5e62a07b67ec69247d7bab29690cb8af64243fd6feb0e29e886`;
  Fedora
  `a4c958c659abe73ad1f937a4187d0d301408ecca0e1d9b2279d9fed99c01caef`.
- GUI-tested: no new GUI claim belonged to Slice 1.3; at that checkpoint the
  earlier clear-color GTK/X11 desktop smoke was the current GUI evidence.
- Package-tested: the relocatable release tree is verified, but no `.deb`,
  RPM, installer, or clean desktop install is claimed.

### 2026-07-23 — Linux desktop VM and GTK/OpenGL environment proof

- Created `kitmux-linux-desktop`, a separate Ubuntu 26.04 ARM64 Lima VZ VM
  with 4 CPUs, 8 GiB RAM, a 32 GiB disk, video enabled, and only this
  workspace mounted writable.
- Installed XFCE 4.20, TigerVNC/noVNC, GTK 4.22.4, Mesa, Weston, Xwayland, and
  the build toolchain. The local-only noVNC desktop is forwarded to host port
  6080.
- Compiled and launched the project-owned GTK 4 `GtkGLArea` smoke and an XFCE
  terminal. `xdotool` found the visible GTK window and `scrot` captured
  `kitmux-linux/desktop-vm-proof.png`.
- `test-desktop.sh` passed X11, noVNC, GTK compilation, and OpenGL discovery.
  The renderer is Mesa llvmpipe. This is visible X11/software-rendering proof,
  not physical-GPU, GNOME, Wayland, IME, scaling, or package evidence.

### 2026-07-23 — Linux stress and release-layout proof

- Added a Linux-native gate with 16 simultaneous terminal floods, 736,272
  pumped bytes, exit callback/status validation, 24 forced-close cycles,
  direct-child reaping, zombie absence, and exact FD-baseline restoration.
- Full headless gate passed: six C/C++/ELF/engine/session/stress tests plus the
  Rust/C layout check. CTest completed in 6.58 seconds.
- Built a release-shaped engine tree with pinned CPython 3.14, Kitty,
  libkitty glue/config/font assets, bundled shared libraries, relative ELF
  runpaths, SHA-256 inventory, and no `LD_LIBRARY_PATH`.
- The release stress gate passed from both its random staging path and final
  path. It resolved `libpython3.14.so.1.0` inside the bundle and rejected
  embedded paths to this developer checkout.
- The third-party inventory has started but is explicitly incomplete. Clean
  Ubuntu/Fedora reproduction and full licensing/SBOM were still required at
  this checkpoint.

### 2026-07-23 — clean Ubuntu and Fedora release reproduction

- Added rootless Podman gates that archive the committed Linux tree, add only
  the locked materialized source inputs, and build in fresh Ubuntu 26.04 and
  Fedora 44 ARM64 userspaces. Each distribution passed twice.
- Every pass built the release runtime, ran the 16-session/24-close stress gate
  from a random staging path, moved the complete tree, and passed the gate
  again from its final path.
- Clean isolation found and fixed three hidden dependencies on development
  state: the build-time Python executable borrowed the VM's host
  `libpython3.14`, the default output parent already existed only because of
  earlier builds, and one regular fontconfig library was absent while its
  symlinks survived the VirtioFS extraction.
- The bundled interpreter now has its own relative runpath and reports the
  correct pinned version, CPython 3.14.6. The release audit declares the
  intentionally system-owned X11/XCB/D-Bus dependencies and fails other
  unresolved ELF dependencies.
- Full per-library license attribution and a machine-readable SBOM are the
  remaining Slice 1.3 blocker.

### 2026-07-23 — headless Linux engine proof

- Installed Lima 2.2.0 and created `kitmux-linux`, an Ubuntu 26.04 LTS ARM64
  VM using Apple's virtualization driver. Only this Linux workspace is mounted
  writable.
- Locked macOS commit `e39381a` and Kitty commit `c1d507d`; materialized
  libkitty source and verified seven recorded SHA-256 values.
- Built Kitty's `fast_data_types.so` and X11/Wayland backends on Linux. The
  first build exposed the macOS-only GL loader; the durable Linux transform
  now uses `libGL.so.1` plus `glXGetProcAddressARB` and Kitty's non-Apple sRGB
  capability check.
- Built an ARM64 ELF `libkitty.so` linked to CPython 3.14 with `$ORIGIN`
  runpath and a version script exporting only `kitty_*`.
- The locked macOS session suite runs on Linux after two explicit test
  translations: `/private/tmp` becomes `/tmp`, and Apple's exact stock-zsh
  prompt policy is omitted.
- Passed in the Ubuntu VM:

```sh
cmake -S kitmux-linux -B kitmux-linux/build -DCMAKE_BUILD_TYPE=RelWithDebInfo
cmake --build kitmux-linux/build --parallel 4
ctest --test-dir kitmux-linux/build --output-on-failure
kitmux-linux/scripts/test-rust-header.sh
```

Result: five C/C++/ELF/engine/session tests passed; Rust/C public struct layout
passed. This is headless ARM64 source evidence, not GUI, x86_64, package, or
clean-machine release evidence.

### 2026-07-23 — macOS baseline freeze

- Completed the in-progress `TerminalView` extraction, fixed same-turn
  structural relayout behavior, made GUI smoke failures return a nonzero
  process status, and stabilized control-smoke shell readiness.
- Passed libkitty, Swift smoke, persistence, browser, control, process-close,
  sizing, IME, daily, and stress gates. The source worktree was committed,
  locally preserved app bundles were excluded from status, and the clean
  reference tag was created.

### 2026-07-23 — planning review

- Inspected the original review, the revised plan that replaced it during this
  pass, the macOS handoff and Git status, package manifest, `libkitty`
  build/header/sources, platform imports, tests, and smoke targets.
- Added the agent guide and status ledger.
- No implementation, build, or runtime claim was made.

## Current blockers and limits

- The local desktop VM proves a real GTK/libkitty session over X11 with
  llvmpipe, including physical key press/release/repeat routing and focus
  transfer to an ordinary GTK control. It does not prove performance or driver
  behavior. Wayland, physical GPU, Compose/dead keys/AltGr/non-US layouts/IME,
  selection, clipboard/paste, mouse/wheel, search, fractional or
  mixed-monitor scaling, accessibility, PTY and frame fairness, and WebKitGTK
  coexistence remain untested.
- The GUI keyboard runs assert exact bytes per event but bound, rather than
  fix, the number of auto-repeats: X auto-repeat is the only way to produce a
  real GDK repeat event, and its count is timing-dependent. The fixed
  press/repeat/release expectations live in the display-free key matrix.
- The desktop gate disables X auto-repeat for its keyboard runs and restores
  it on exit. It is a project VNC session control, not something to run
  against a desktop in use.
- The current VM is ARM64. Tier-1 x86_64 proof still requires CI, a remote
  machine, or a separate emulated/native environment.
- Shared portable fixtures are still provisional. Do not start the Rust model
  or claim snapshot/control parity until macOS and Linux consume the same
  valid and invalid fixture files.
- The clean release gate currently proves ARM64 userspaces. Tier-1 x86_64,
  physical-GPU, native package, and clean desktop-install evidence remain
  future gates.
- Fresh clean-gate runtimes were approximately 104 MiB. Ignored local build
  directories may contain older layouts and are not release evidence; always
  build to a new path and run the runtime audit.
- The GTK development host proves the isolated-libpython loader boundary, but
  no release-shaped GUI artifact proves it yet. A global Kitty dependency
  path would shadow GTK's distribution libraries and is forbidden.

## Next-agent handoff

Continue
[Slice 2.2 in the implementation plan](LINUX_PORT_PLAN.md#slice-22-prove-terminal-interaction)
using the exact sequence in [`NEXT_STEPS.md`](NEXT_STEPS.md). Slice 2.2A is
closed; begin with only Slice 2.2B: replace the host's key-symbol text
synthesis in `kitmux_translate_gdk_key` with a real `GtkIMContext`, and prove
Compose, a dead key, AltGr, one non-US layout, emoji, and an IBus
preedit/commit flow. Do not begin product chrome, the Rust model, browser
functionality, packaging, or repository migration.
