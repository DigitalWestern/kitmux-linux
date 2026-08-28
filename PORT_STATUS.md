# Kitmux Linux Port Status

**Last inspected:** 2026-08-27

This file is the evidence ledger and the authority on what is currently true.
The next slice is in [`NEXT_STEPS.md`](NEXT_STEPS.md); phase scope is in
[`LINUX_PORT_PLAN.md`](LINUX_PORT_PLAN.md); commands are in
[`docs/LINUX_DEVELOPMENT.md`](docs/LINUX_DEVELOPMENT.md).

**Implementation state:** Phases 0 through 5 and Slices 6.1–6.3 are closed. GTK 4 is
the selected Linux UI toolkit. What exists: separate Ubuntu ARM64 headless and
XFCE desktop VMs; an ELF `libkitty.so`; a relocatable ~107 MiB attributed engine
runtime; Linux stress tests; a GTK 4 terminal host over X11 and native Wayland
client paths; deterministic keyboard, input-method, and fractional-scaling
harnesses; an authoritative portable contract corpus; a display-free Rust
product model with bounded contracts; a release-shaped terminal multiplexer
alpha; and secure local control plus reviewed SSH connect/reconnect workflows.

The alpha has live hierarchy navigation, nested terminal splits, one permanent
libkitty session per live surface, selection/clipboard/paste-safety/mouse/wheel/
search, native command and settings controls, an in-window menu bar (its
keyboard-traversal gate check is still open), full safe hierarchy persistence,
close-chain foreground review, and initial accessible roles and focus order.
The app crate is modularized (2026-08-27) with no intended behavior change.
Reproducible ARM64 tarball and `.deb` artifacts now exist and have passed a
fresh-VM lifecycle gate; promotion is still blocked by the remaining hardware,
architecture, CI, and threat/soak evidence below.

**Next gate:** Physical-Mesa GPU rendering and interaction, followed by x86_64
and release-readiness evidence. Remote CI (ADR 0008 R2) closed 2026-08-27.

**Non-claims:** see [Current blockers and limits](#current-blockers-and-limits).
All GUI evidence below is ARM64, Mesa llvmpipe, one machine, one person.

**Licence:** GPL-3.0-only, decided 2026-07-25 in ADR 0006. `LICENSE` is at the
repository root.

## Verified checkout state

Adjacent macOS checkout: `../macos/kitmux/`

### 2026-08-27 — First remote CI pass; ADR 0008 R2 closed

- `.github/workflows/linux-standalone.yml` run 33131978184 at `3a4aa88`
  passed both jobs on GitHub-hosted runners. standalone-arm64
  (`ubuntu-24.04-arm`) ran the full standalone gate from a clean checkout:
  locked materialization, kitty build, 6/6 ctest (headers, ELF audit,
  engine lifecycle, session API, session stress), Rust header layout,
  portable fixtures, and the complete model gate. standalone-amd64 passed
  the explicitly unlocked x86_64 fallback build; per ADR 0008 that remains
  fallback smoke evidence, not an x86_64 support claim.
- Reaching green surfaced five defects that only a clean host could show —
  the VMs' persistent `.source` tree and CMake caches had been supplying
  state a fresh checkout lacks:
  1. The workflow was missing xinerama/xcursor/xi/Wayland dev packages and
     used Ubuntu's apt cargo 1.75, which cannot build edition-2024 crates;
     it now installs the provision script's package set, zsh (the session
     foreground tests spawn `/bin/zsh`), and pinned rustup 1.97.1 with
     rustfmt and clippy.
  2. `build-kitty-dev.sh`'s locked path never extracted
     `SymbolsNerdFontMono-Regular.ttf`, so kitty's setup fell back to
     fc-list and failed.
  3. The locked path also never relocated the bundle's `/sw/sw` build
     prefix in `.pc`/`_sysconfigdata` files, so compiles used nonexistent
     include paths; extraction now repeats kitty's relocation and drops the
     bundled libfontconfig.
  4. Bare `find_package(Python3)` linked the host libpython while
     PYTHONHOME pointed at the bundle, failing `Py_InitializeFromConfig`;
     CMake now pins the bundled embed artifacts directly (no interpreter
     probe) and uses the host python3 only for codegen.
  5. Linking the bundled libpython leaked its directory into libkitty's
     runpath, failing the ELF audit's exact `[$ORIGIN]` check; the build
     now uses the install rpath plus a link-time-only INTERFACE
     `-rpath-link` for consumers.
- Every fix was verified before push on a clean `git clone` inside the
  ARM64 headless VM (6/6 ctest, runpath exactly `[$ORIGIN]`) and the
  shared-mount `test-headless.sh` end to end, so the existing VM flow is
  unchanged.

### 2026-08-27 — App crate modularized; no behavior change intended

- Split `rust/app/src/main.rs` (5,789 lines) into focused modules with a
  52-line `main.rs`: `terminal.rs` (engine/session runtime, PTY pump,
  `impl Terminal`), `control.rs` (control-socket server and dispatch),
  `navigation.rs` (navigation UI sync and gate drivers), `dialogs.rs`
  (modals, palette execution, paste/close review), `menu.rs` (menu bar and
  actions), `ssh.rs` (ssh -G resolution), `restore.rs` (snapshot
  restoration), `runtime.rs` (runtime bundle, account, C-string helpers).
  `window.rs` imports were updated; moved items became `pub(crate)`. The
  only non-mechanical edits were the three outstanding Clippy fixes
  (two `needless_borrow` in the menu actions, one `collapsible_if` in
  `split_layout`) and moving the control-surface test module below the items
  it tests. Doc pointers in `docs/MENUBAR_PLAN.md` were updated.
- Verified on this macOS host: function and type name multisets before/after
  the split are identical; `cargo fmt -- --check` passes for app and model;
  `cargo clippy --locked --all-targets -- -D warnings` passes for the app
  crate with GTK4 pkg-config probing bypassed via `SYSTEM_DEPS_*_NO_PKG_CONFIG`
  overrides and a stub `KITMUX_NATIVE_LIB_DIR` (typechecks against the pinned
  gtk4-rs 0.11.4 API; does not link or run); the model suite passed
  (4 unit, 29 contract, 9 interaction, 18 model, 5 persistence tests).
- ARM64 VM evidence, same date: in the headless VM,
  `CARGO_NET_OFFLINE=true scripts/test-model.sh` passed (fmt, clippy
  `-D warnings`, full suite including the 9 Linux-only control-socket
  tests). In the desktop VM on `DISPLAY=:1` (X11, Mesa llvmpipe), the
  refactored app built into a fresh release runtime and
  `test-phase5-navigation.sh`, `test-phase6-control.sh`,
  `test-phase6-resume.sh`, `test-phase6-ssh.sh`, and `test-phase4.sh` all
  passed — covering the moved navigation, control-dispatch, resume-dialog,
  SSH, paste/close-review, and session-runtime code against the live app.
- `test-phase5-product.sh` failed twice at the same point: the app reaches
  `accessibility_ready roles=true focus=true` and emits `menu_key key=F10`,
  but the gate never observes the arrow/Escape `menu_keyboard_traversal`
  diagnostic. This is the same failure recorded in the 2026-08-26 menu-bar
  closeout, in `window.rs` logic the refactor did not modify (imports only);
  it remains the open menu-traversal item, now known to be deterministic
  rather than an interrupted run. Package, clean-machine, physical-GPU, and
  x86_64 evidence remain unclaimed.

### 2026-08-26 — Approved Linux menu-bar build Slice 7.1, contract and model slice

- The approved Linux-first menu bar is a beta surface but does not replace or
  delay the physical-GPU, remote-CI, x86_64, signing, vulnerability, or other
  release-readiness gates. It is visible by default. New Window and SSH
  connect are intentionally rendered disabled until their prerequisites are
  implemented. Help uses the existing Kitmux website URL; Report an Issue uses
  the public issue destination documented in `docs/MENUBAR_PLAN.md`.
- Added the `desktop.menu-bar` area and File/Edit/View/Window/Help rows, and
  amended `macos-only.appkit-surfaces` so it no longer calls Linux menu bars
  omitted. The inventory contract validates after this entry is recorded.
- The command catalog is now 49 IDs: the ten Linux menu commands are appended
  in deterministic order, with semantic mappings, fixture coverage, and the
  count assertion updated. `cargo test --manifest-path
  kitmux-linux/rust/model/Cargo.toml` passed 4 unit, 29 contract, 9
  interaction, 18 model, and 5 persistence tests.
- Menu GTK/app Clippy/product-gate evidence is still pending on the ARM64
  desktop VM. A host `cargo check` is not authoritative here: this macOS host
  has no GTK4 pkg-config development files and no `KITMUX_NATIVE_LIB_DIR`.

### 2026-08-26 — Menu-bar closeout handoff

- Implemented the GTK 4 in-window File/Edit/View/Window/Help menu surface in
  `rust/app`, backed by the existing command catalog, shortcut map,
  `palette_command_supported`, and navigation dispatch. Added dynamic
  workspace/tab menu sections, overflow labels, disabled New Window and SSH
  entries, foreground close review routing, persisted visibility, and the ten
  requested command IDs. The per-agent menu remains intentionally unbuilt.
- Updated `docs/MENUBAR_PLAN.md`, `docs/LINUX_DEVELOPMENT.md`, the inventory,
  command/settings fixtures, model tests, and this handoff. The stale
  macOS-only inventory claim is corrected; no macOS checkout was edited.
- Observed passing: `git diff --check`; app `cargo fmt -- --check`; model test
  suite (4 unit, 29 contract, 9 interaction, 18 model, 5 persistence tests);
  and `python3 contracts/validate-inventory.py` (`70 features across 18
  areas`, `inventory OK`). The ARM64 release runtime also built and passed its
  existing relocation, metadata, flood, forced-close, and FD checks.
- The final ARM64 `test-phase5-product.sh` menu traversal gate is not claimed
  as passed. The app reached `accessibility_ready roles=true focus=true` and
  emitted the F10 diagnostic, but the gate did not observe the complete
  arrow/Escape traversal before the closeout stop. The last retry ended with
  exit 141 before producing a new gate result. ARM64 GUI evidence remains
  X11/Mesa llvmpipe; physical GPU, remote CI, x86_64, signing, clean-machine,
  and vulnerability proof remain open release boundaries.

### 2026-08-23 — Local audit fixes and complete aggregate gate passed

- Fixed the restored-cwd root cause in `rust/app`: a missing or invalid saved
  cwd now launches from the home fallback without being falsely reported as a
  restored cwd. The current surface schema is covered by the persistence gate;
  the persistence and bash/zsh/fish/vim/less/tmux gates both passed after the
  fix. `cargo fmt --manifest-path kitmux-linux/rust/app/Cargo.toml -- --check`
  also passed.
- The Kitty development wrapper now repairs dependency trees only from a
  hash-matching locked archive and removes the known dangling fontconfig links
  left by the explicitly unlocked x86_64 fallback. The fallback remains
  intentionally non-reproducible. Clean VM provisioning now installs the
  locked toolchain's `rustfmt` and `clippy` components.
- `KITMUX_DISPLAY=:1 bash kitmux-linux/scripts/test-all.sh` passed end to end:
  the 1,800-second soak completed 1,089 iterations, 161 heartbeats, and a
  340 ms maximum heartbeat; Phase 5 product/navigation, Slice 6 control,
  resume, SSH, release, package, clean-target, and clean-container gates all
  passed. The final result was `All Kitmux Linux gates: OK`.
- The aggregate verified the relocatable runtime and `share/SHA256SUMS`, ran
  two stable Ubuntu clean-release passes with inventory
  `d0db246996abdf4e24570b4bb3cccd8adec5ccf312603761c5f1e6985f0b3d49`, and two
  stable Fedora passes with inventory
  `a105cca8b134d9b1e3c7a15a9b25284b958d6246dd42bf5577d361d9596f4455`.
  The ARM64 package lifecycle also passed; its aggregate tarball and Debian
  package hashes were `496115f1e1ad46cd16601987b4509771e2bb4125a655457c16a142e5feb16449`
  and `a1f0bcf5e04fc4af94e24f447452518b4044a54b383099a27e2c4fe357c4dc8a`.
- The dirty macOS reference worktree's complete `make -C
  ../macos/kitmux/libkitty test` suite now passes, including the former
  foreground-process failure. This is not a clean macOS rebaseline: relevant
  macOS files remain uncommitted, so reference drift stays blocked. The final
  drift report recorded current macOS `HEAD` `7daea1d6e18862447bc69b1f0cd73c3b1184e2ed`,
  seven relevant committed files, and two relevant uncommitted files.
- Current GUI evidence remains Ubuntu ARM64 X11 under Mesa llvmpipe
  (`llvmpipe (LLVM 21.1.8, 128 bits)`), not physical-GPU evidence. The x86_64
  compact fallback standalone smoke passes, but the exact locked dependency
  bundle is still unavailable; no x86_64 reproducibility or package claim is
  made. This checkout has no Git remote, so remote CI was not triggered.
- Source-tested, GUI-tested, release-runtime-tested, package-lifecycle-tested,
  clean-target-tested, and clean-container-tested: yes as stated above.
  Physical-GPU, remote-CI, exact x86_64 bundle/package, real SSH/network
  authentication, power-loss, signing, vulnerability, full AT-SPI, and
  desktop-menu interaction evidence remain open.

### 2026-08-19 — Terminal-beta parity scope reviewed

- `python3 contracts/validate-inventory.py` passed with 135 Linux references,
  234 macOS references at the locked baseline, and 64 features across 17 areas.
- The inventory now classifies 54 rows as terminal-alpha, 7 as beta, 2 as
  later, and 1 as intentionally omitted. Browser panes and desktop
  notifications are explicitly later under the approved terminal-only beta;
  this is scope deferral, not implementation evidence.

### 2026-08-18 — Reference drift rechecked

- `kitmux-linux/scripts/report-reference-drift.py` found frozen tag commit
  `3088295003c0842d7c3198102d0d05378da4dc62` and current macOS `HEAD`
  `dd798393c861e11883d4e215da181fc619c00ce9`.
- Six relevant committed files have drifted: one contract-affecting header and
  five behavior-affecting or unrelated files. Two relevant uncommitted macOS
  files remain (`libkitty/py/glue.py` and `macos/KitmuxApp/Sources/Kitmux/PaneRuntime.swift`).
- Rebaselining remains intentionally blocked until those macOS changes are
  clean and reviewed; no macOS files were changed here.

### 2026-08-18 — Slice 6.3 resume and recovery closed

- Resume metadata is inert: terminal user-variable delivery accepts only the
  bounded validated command text and persists it as metadata; restoring a
  terminal creates a fresh shell, and restoring an SSH pane creates a
  disconnected placeholder with its profile UUID only. No saved argv, PID,
  runtime handle, agent socket, or SSH session is persisted or auto-started.
- Saved commands appear in a native review dialog with every checkbox off.
  Release builds have no environment-controlled approval path. The explicit
  Run action rechecks pane identity, unchanged command text, unchanged cwd,
  eligibility, and non-SSH status immediately before writing the command and
  carriage return to the live terminal. A changed identity is skipped.
- Source evidence: `kitmux-linux/scripts/test-model.sh` passed 3 unit, 28
  contract, 8 interaction, 18 model, and 5 persistence tests. The ARM64 VM
  app Clippy gate passed with `-D warnings` and the release native bridge.
- Release/GUI evidence: `limactl shell kitmux-linux-desktop -- env DISPLAY=:1
  bash /Users/ethanabbate/Desktop/System/home-kitmux/operating-system/linux/kitmux-linux/scripts/test-phase6-resume.sh`
  passed. The fresh release runtime verified 2 locked bundles, 21 source
  archives, 2 Cargo lockfiles, 25 bundled and 10 system SONAMEs, a 24-package
  SPDX SBOM over 895 files, the 16-session flood, 24 forced-close cycles, and
  restored FD baseline. The gate covered unchecked decline, explicit approved
  execution, identity-race rejection, SIGTERM save, SIGKILL stale-socket
  replacement, `.last-good` preservation, disconnected SSH restore with no
  fake-SSH invocation, legacy v1 pane metadata upgrade, and three repeated
  inert startup/close cycles.
- Phase 7 decision: the target is a terminal-only beta. Browser panes, portal
  integrations, and their additional X11/Wayland safety tests are deferred;
  the existing WebKit coexistence probe is not being promoted to product
  functionality.
- Limits: this is Ubuntu 26.04 ARM64 X11 under Mesa llvmpipe. It does not
  prove a physical GPU, real SSH network/authentication, x86_64, native Wayland
  resume UI, or power-loss durability.
- Source-tested, GUI-tested, release-runtime-tested, and upgrade-compatibility
  tested: yes as stated above. Physical-GPU-tested, x86_64-runtime-tested,
  network/auth-tested, and power-loss-tested: not run.

### 2026-08-18 — Slices 1–6 audit follow-up

- Startup errors now carry the libkitty 1024-byte diagnostic through the GTK
  status line and structured `terminal_init_failed` event. The navigation
  callbacks return safely when their optional model is absent; shutdown is
  idempotent across close-request and unrealize; the unused bridge exports and
  committed-state placeholder are gone. The mouse wheel default is a bounded
  Linux setting, defaulting to three lines and surviving settings reloads.
- The feature inventory backfilled eleven previously blank Linux rows, kept
  three genuinely unproven/omitted rows explicitly empty, and now resolves 135
  Linux references, 234 macOS references, and all 64 features. Every one of
  the 21 current `test-*.sh` gates sources the shared validator. The new
  `test-all.sh` prints and runs the dependency-ordered host/headless/desktop
  sequence.
- Release and development CMake defaults now use `/tmp` (with environment
  overrides), and the release builder refuses an existing output tree. The
  ignored conflict-heavy `build-release/` tree and old runtime containing
  39 MiB of `libslang` were moved, recoverably, to
  `/tmp/kitmux-linux-audit-quarantine.TzTQmU`; they are not release evidence.
  Fresh runtime audit output was `109712 KiB` with `libslang payload: 0 bytes`.
  The runtime guard now fails if any `libslang*` file is present.
- `test-model.sh` and `test-phase3.sh` now run under Bash with
  `pipefail`. The ARM64 headless standalone gate passed all six C/ELF/session
  tests plus the Rust and fixture gates. The ARM64 desktop resume gate built
  the GTK app and passed the explicit-review, identity-race, stale-socket,
  crash-safe persistence, SSH-placeholder, and stress checks. App Clippy passed
  with `-D warnings`; a combined CMake configure generated only the app
  `lib/app/libkitty.so` install entry.
- Source-tested, GUI-tested, release-runtime-tested, and standalone-tested:
  yes as stated above. Native-package-tested, clean-desktop-installed,
  x86_64-runtime-tested, physical-GPU-tested, real network/auth-tested, and
  power-loss-tested: not run.

### 2026-08-18 — Physical Mesa GPU gate remains blocked

- `limactl shell kitmux-linux-desktop -- bash -lc 'DISPLAY=:1 glxinfo -B'`
  reports `OpenGL renderer string: llvmpipe (LLVM 21.1.8, 128 bits)` and Mesa
  26.0.3. `lspci` identifies the display device as `Red Hat, Inc. Virtio 1.0
  GPU`, not a host physical GPU. The headless VM has no display device.
- This is valid ARM64 X11 correctness evidence only. No rendering or
  interaction result from these VMs can close the physical-GPU beta gate. A
  Linux host or VM with a hardware-backed Mesa renderer is still required.

### 2026-08-18 — Standalone source gate passed; R1 closed

- `limactl shell kitmux-linux -- bash
  /Users/ethanabbate/Desktop/System/home-kitmux/operating-system/linux/kitmux-linux/scripts/test-standalone.sh`
  passed with `KITMUX_MACOS_REPO` pointed at a deliberately missing path. The
  gate materialized the durable libkitty mirror, verified the ARM64 and font
  bundle hashes, reused the pinned Kitty commit, rebuilt the headless engine,
  passed all 6 CTest cases, the Rust/C header layout check, 6 fixture
  contracts, and the model suites: 3 unit, 29 contract, 9 control-socket, 8
  interaction, 18 model, and 5 persistence tests.
- This closes ADR 0008 R1 for the current ARM64 source path: the Linux tree
  no longer requires the adjacent private macOS checkout to materialize or
  build its locked reference. R2 is not closed because the workflow has not
  run on a remote CI service in this repository.

### 2026-08-18 — ARM64 package artifacts and fresh-VM lifecycle passed

- `package-tarball.sh` produced the reproducible ARM64 artifact
  `/tmp/kitmux-0.1.0-arm64-v3.tar.xz`, SHA-256
  `ea0f9cfa502cfc1dea2c279191a7dcdb8f72cf8847ef4d6720ab16d5a61ee12c`.
  `package-deb.sh` produced `/tmp/kitmux_0.1.0_arm64-v3.deb`, SHA-256
  `dadeb5c896d67eb501c49172b519be4beca5b4b5600fae3dadda9fc45c5440f9`.
  Rebuilding both from the same release runtime produced byte-identical files.
- On fresh VM `kitmux-linux-package-20260818`,
  `test-package-lifecycle.sh` passed tarball launch, Debian install and launch,
  upgrade to 0.1.1, downgrade to 0.1.0, reinstall, and uninstall. The test
  used the installed non-root launch path and verified the desktop entry and
  removal of `/usr/bin`, `/usr/lib/kitmux`, and the desktop file.
- This is ARM64 Ubuntu 26.04 package evidence. It does not close x86_64,
  remote-CI, signing, vulnerability, desktop-menu interaction, or physical-GPU
  release gates.

### 2026-08-18 — Accessibility, threat review, and clean-VM soak

- `limactl shell kitmux-linux-desktop -- env DISPLAY=:1
  bash .../test-phase5-product.sh` passed the product controls, close review,
  persistence round trip, corrupt-primary `.last-good` recovery, accessible
  roles, and focus transfer gate.
- `docs/THREAT_MODEL.md` records the terminal-alpha assets, trust boundaries,
  controls, exact security gates, and residual risks. Existing control-socket,
  SSH, diagnostics, resume, and release-runtime gates provide the referenced
  evidence; the document does not claim sandboxing or real SSH authentication.
- The first 30-minute soak attempt reached 1,347 seconds before shell heartbeat
  126 exceeded the five-second bound. After a clean desktop-VM restart, the
  same command passed: `Phase 4 30-minute flood/resize/interaction soak: OK
  (1800s, 1085 iterations, 162 heartbeats, 320ms max heartbeat)`. The gate's
  cleanup now tolerates the known portal-FUSE cache mount and disables portal
  selection for the isolated run.
- This closes the current ARM64 X11 accessibility and soak evidence only. The
  first-run late heartbeat remains a VM-stability observation; it is not hidden
  as a pass. Physical GPU, x86_64, remote CI, package signing/vulnerability,
  complete AT-SPI, and power-loss evidence remain open.

### 2026-08-18 — Short soak smoke attempt not accepted

- A 60-second retry was attempted after starting the documented TigerVNC
  display. It reached `soak progress: 60/60s` but exited without an `OK` result.
- A traced 10-second run identified the short-run assertion: only one pointer
  event and zero scroll events were observed against a minimum of three. This
  is not a soak pass and does not replace the recorded 1,800-second result.

### 2026-08-18 — Confirmed audit findings remediated

- The clean-container gate now passes an explicit output path,
  `kitmux-linux/build/kitmux-engine-runtime`, which is the same tree whose
  `share/SHA256SUMS` file it hashes. The aggregate `test-all.sh --list` path
  exits before inventory validation; the orchestrator passes the validated
  marker through host, Lima, nested, and Podman gate processes so the shared
  inventory check is not repeated unnecessarily.
- The durable reference mirror now contains the complete 17-file input tree,
  including the Makefile, private header, examples, and tests. Its complete
  tree digest is locked in `source-lock.json`; materialization rejects symlinks,
  verifies the tree digest, then verifies the seven named file hashes and Linux
  overlay. The ARM64 and Nerd Font dependency bundles are mirrored and hashed.
  The historical x86_64 bundle remains intentionally unavailable: x86_64 has
  an explicit unlocked fallback build and CI job, but no x86_64 reproducibility
  or support claim.
- Release dependency closure now includes `kitmux` and `kitmuxctl` when the
  application runtime is built. The fallback runtime component owns otherwise
  unassigned bundled libraries, and `test-all.sh` includes the tarball/.deb
  lifecycle gate. The current ARM64 rerun passed release closure (31 bundled,
  16 system SONAMEs), SPDX ownership (25 components, 901 files), tarball and
  Debian packaging, launch, upgrade, downgrade, reinstall, and uninstall.
- macOS pending PTY input is discarded at child reap/close and the Swift retry
  loop stops when the child exits. The new `libkitty/tests/test_pending_write.py`
  regression passed. SwiftPM passed 292 tests with 1 skipped and 0 failures.
  The broader C target is currently blocked by the pre-existing foreground-
  process assertion in `libkitty/tests/test_session.c:113`; the targeted new
  regression is independent and passes.
- Resume offers and final validation are keyed by `SurfaceId`, not `PaneId`.
  The ARM64 two-surface fixture shows two rows in one pane, selective restore,
  restore-all, and identity-race rejection. The current resume/recovery gate
  passed, as did shell syntax, JSON/Python checks, Rust formatting, app Clippy
  with `-D warnings`, and the complete durable-reference materialization check.

### 2026-08-17 — Slice 6.2 SSH and agent workflows closed

- Added a bounded Linux SSH profile store at the XDG config path, with an
  absolute `KITMUX_SSH_PROFILES_PATH` override for gates. Existing profile files
  must be user-owned, non-symlink regular files, and private; writes are atomic
  `0600` replacements. Duplicate names, invalid profiles, oversized documents,
  corruption quarantine, concurrent creates, approval persistence, and
  edit-driven approval clearing are covered by the model tests.
- Added real executable discovery through `PATH`, bounded `ssh -G -- host`
  resolution with a five-second deadline and 512 KiB output bound, separated
  stdout/stderr draining, and generic failure messages that do not carry
  stderr or expanded private arguments into diagnostics. SSH sessions receive
  `SSH_AUTH_SOCK` when present; missing agent state is reported only as a
  boolean.
- Added exact-UUID `kitmuxctl ssh profile list`, `ssh connect`, and
  `ssh reconnect` routes. Connect returns a fingerprinted review before an
  externally listening forward can launch. Reconnect requires a disconnected
  SSH pane and performs the same explicit review/approval before replacing the
  dead runtime. Launches use an absolute argv with the remote command as one
  argument and never pass through a shell.
- Source evidence: `limactl shell kitmux-linux -- env
  CARGO_NET_OFFLINE=true "$PWD/kitmux-linux/scripts/test-model.sh"` passed
  model Clippy/formatting, 28 contract, 9 control-socket, 8 interaction, 18
  model, and 5 persistence tests; the duplicated Linux control-socket run also
  passed 9 tests. App Clippy passed with `-D warnings` against the ARM64 native
  bridge libraries.
- Release/GUI evidence: `limactl shell kitmux-linux-desktop -- env DISPLAY=:1
  "$PWD/kitmux-linux/scripts/test-phase6-ssh.sh"` passed with a fresh
  release-shaped runtime, 25 bundled and 10 system SONAMEs, a 24-package SPDX
  SBOM, the 16-session flood, 24 forced-close cycles, and the SSH acceptance
  checks for nonstandard executable lookup, review/approval, explicit
  reconnect, one-argument remote commands, missing agent environment, private
  profile mode, and log safety. This is Ubuntu 26.04 ARM64 X11 with Mesa
  llvmpipe.
- Contract/inventory evidence: `python3 contracts/validate-fixtures.py` passed
  (6 contracts, 20 cases, 7 versioned files), and
  `python3 contracts/validate-inventory.py` passed (111 Linux references, 234
  macOS references, 64 features across 17 areas). The locked materialization
  check passed: 7 reference files and 1 Linux overlay.
- Phase-boundary drift: `kitmux-linux/scripts/report-reference-drift.py`
  passed with the frozen tag and current macOS `HEAD` both at
  `3088295003c0842d7c3198102d0d05378da4dc62` and no relevant committed drift.
  Rebaselining remains blocked by five relevant uncommitted macOS files; no
  macOS files were changed here.
- Limits: the acceptance executable is a deterministic nonstandard-path test
  harness, so this slice does not prove a real remote host, network
  authentication, host-key interaction, a desktop agent actually signing, or
  profile authoring UI. SSH pane metadata is not restored across app launches;
  that belongs to Slice 6.3. Native Wayland SSH, physical-GPU behavior,
  packaging, clean-machine installation, x86_64, and native package evidence
  remain unproven.
- Source-tested, GUI-tested, and release-runtime-tested: yes as stated above.
  Native-package-tested, clean-desktop-installed, x86_64-runtime-tested,
  physical-input-tested, physical-GPU-tested, and network/auth-tested: not run.

### 2026-08-02 — Slice 6.1 secure local control and CLI audit closed

- Added the Linux control server and CLI over a bounded newline-delimited
  protocol: private XDG socket resolution, `0600` mode, owner/type/symlink
  checks, Linux `SO_PEERCRED`, bounded frames, and a bounded event history with
  cursor filtering.
- Added release-runtime `kitmuxctl` installation and
  `scripts/install-user-cli.sh`; the final desktop gate exercised the
  user-local fallback and its missing-source diagnostic.
- `kitmux-linux/scripts/test-model.sh` passed on macOS and in the Ubuntu ARM64
  headless VM. The current Linux run passed 26 contract, 9 control-socket, 8
  interaction, 17 model, and 4 persistence tests; the CLI parser test is
  `cli_parser_maps_bounded_commands_without_shell_strings`.
- `KITMUX_BUILD_APP_RUNTIME=1 KITMUX_APP_TEST_HOOKS=ON
  kitmux-linux/scripts/build-release-runtime.sh` passed in the Ubuntu ARM64
  desktop VM, including dependency closure, SPDX SBOM, release metadata, the
  16-session flood, 24 forced-close cycles, and FD restoration.
- The final `DISPLAY=:1
  kitmux-linux/scripts/test-phase6-control.sh` gate passed socket
  mode/ownership/type, ping/identify/tree, hierarchy and pane dispatch,
  event peer identity, idle-client responsiveness, malformed/oversized errors,
  multiple-instance ownership, stale replacement, default XDG resolution,
  user-local CLI installation and missing-source diagnosis, and symlink
  refusal. The headless Rust socket tests separately prove the client cap and
  total deadline behavior.
- Known limitation: if the runtime socket file is removed while the server is
  still running, restart Kitmux or set `KITMUX_SOCKET_PATH`; no auto-rebind
  watcher is implemented.
- Task 14 clean app-runtime builds now share `build-release/cargo-app`: both
  `kitmux` and `kitmuxctl` were produced, with no `cargo-cli` directory. The
  measured split-target baseline was 50.845s real and the shared-target build
  was 50.668s real (0.177s / 0.35% faster); this is not a material performance
  claim, but confirms the shared-target build path.
- Source-tested: yes on macOS and Ubuntu ARM64; GUI/release-runtime-tested:
  Ubuntu 26.04 ARM64 X11 with Mesa llvmpipe. Native-package-tested,
  clean-desktop-installed, x86_64-runtime-tested, physical-input-tested, and
  physical-GPU-tested: not run. Slice 6.2, SSH, resume, packaging, and
  physical-Mesa proof remain open.

### 2026-08-17 — reliability remediation verified

- `save_state` now replaces a corrupt primary snapshot while retaining a valid
  `.last-good` backup when one exists. The app installs a SIGTERM flag handler
  and the GTK main loop performs the normal state save and session reap before
  closing. The control gate's intentionally abrupt-death checks use SIGKILL;
  graceful SIGTERM shutdown is tested separately.
- Rename dialogs capture workspace, group, and tab IDs. Palette, settings,
  rename, and close dialogs share one modal-mutation guard; settings also refuse
  to overwrite a document changed while the dialog was open. Control-client
  thread-spawn failure now releases its reserved client slot.
- `cargo test --manifest-path kitmux-linux/rust/model/Cargo.toml --locked`
  passed on macOS and in the Ubuntu ARM64 desktop VM. The VM run passed 1 unit,
  28 contract, 9 control-socket, 8 interaction, 18 model, and 5 persistence
  tests; model Clippy with warnings denied passed.
- In the Ubuntu ARM64 desktop VM, `cargo check --manifest-path
  kitmux-linux/rust/app/Cargo.toml --locked` passed with the release native
  library path. `KITMUX_BUILD_APP_RUNTIME=1 KITMUX_APP_TEST_HOOKS=ON
  kitmux-linux/scripts/build-release-runtime.sh /tmp/kitmux-fix-runtime-20260817`
  passed, as did `DISPLAY=:1 kitmux-linux/scripts/test-phase5-product.sh` and
  `DISPLAY=:1 kitmux-linux/scripts/test-phase6-control.sh`.
- A live release-runtime SIGTERM smoke with temporary XDG roots observed
  `sigterm_shutdown`, `state_saved`, and shutdown with two sessions after a
  control-created second workspace. This proves the graceful termination path,
  not persistence after SIGKILL, a crash, or power loss.
- Source-tested: yes on macOS and Ubuntu ARM64; GUI/release-runtime-tested:
  Ubuntu ARM64 X11 with Mesa llvmpipe; current app and model Clippy with
  `-D warnings` passed. Native packaging, clean-desktop installation, x86_64
  runtime, and physical-GPU evidence remain unproven.

Clean reference revision:
`3088295003c0842d7c3198102d0d05378da4dc62`
(`docs: clarify GL context transfer lifecycle`)

Reference tag:
`macos-linux-port-baseline-2026-08-02-v0.21`

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
- GTK 4 is the selected toolkit. The disposable C spike proved the direct
  libkitty/GTK boundary; Rust owns the display-free product model. The Phase 4
  shell should reuse the proven contracts without promoting the spike host to
  production architecture.
- The first user-facing target is a terminal-only alpha. Browser panes and
  broad distribution support are later decisions.

## Evidence log

### 2026-08-02 — Slices 0–5 maintenance audit verification complete

- `git status --short` returned empty after the final task commit.
- `python3 contracts/validate-fixtures.py` passed: 6 contracts, 20 cases,
  7 versioned JSON files; portable fixtures OK.
- `python3 contracts/validate-inventory.py` passed: 97 Linux test references,
  234 macOS source and test references, 64 features across 17 areas; inventory OK.
- `kitmux-linux/scripts/test-model.sh` passed 52 Rust model/contract/
  interaction/persistence tests with formatting and Clippy clean.
- `kitmux-linux/scripts/test-phase3.sh` passed 52 Rust tests, 6 contracts/20
  cases/7 byte-identical fixture files, 97 Linux inventory references, 234 macOS
  references, and all 9 macOS portable-contract consumer tests. The first
  sandboxed attempt was blocked by SwiftPM's `sandbox-exec: sandbox_apply:
  Operation not permitted`; the same command passed when rerun with host
  permissions.
- `kitmux-linux/scripts/materialize-reference.sh` passed: 7 locked reference
  files and 1 Linux overlay verified.
- `kitmux-linux/scripts/report-reference-drift.py` passed: frozen tag and
  current macOS HEAD both resolve to
  `3088295003c0842d7c3198102d0d05378da4dc62`; no relevant committed or
  uncommitted drift.
- `limactl shell kitmux-linux -- "$PWD/kitmux-linux/scripts/test-headless.sh"`
  passed all 6 C/C++/ELF/engine/session/stress tests. The guest skipped its
  reference check because the macOS checkout is not mounted in the headless VM;
  host materialization and drift checks above passed.
- `limactl shell kitmux-linux-desktop -- env DISPLAY=:1
  "$PWD/kitmux-linux/scripts/test-phase4.sh"` passed: Phase 4 release-layout
  lifecycle and terminal-interaction gate OK.
- `limactl shell kitmux-linux-desktop -- env DISPLAY=:1
  "$PWD/kitmux-linux/scripts/test-phase5-product.sh"` passed: Phase 5 product
  controls, close review, persistence, and accessibility gate OK.
- `limactl shell kitmux-linux-desktop -- env DISPLAY=:1
  KITMUX_RAPID_NAV_GATE=1 "$PWD/kitmux-linux/scripts/test-phase5-navigation.sh"`
  passed: Phase 5 rapid navigation and permanent-session churn gate OK.
- `limactl shell kitmux-linux-desktop -- env DISPLAY=:1
  "$PWD/kitmux-linux/scripts/test-phase4-wayland.sh"` passed: Phase 4
  release-layout lifecycle and terminal-interaction gate OK, and native-Wayland
  product interaction gate OK.
- Source-tested: yes on macOS and Ubuntu ARM64, including cross-host fixtures.
  GUI-tested: Ubuntu 26.04 ARM64 X11 and native Wayland with Mesa llvmpipe.
  Release-layout-tested: yes through fresh 894-file runtimes.
  Native-package-tested, clean-desktop-installed, x86_64-runtime-tested,
  physical-input-tested, and physical-GPU-tested: none. Slice 6.1 is next; no
  Phase 6 product work was started.

### 2026-08-02 — macOS/libkitty v0.21 reference rebaseline complete

- The phase-boundary report found mandatory public-header and patch drift for
  additive `kitty_session_release_render_resources` plus its multi-context
  renderer lifecycle. A follow-up replaced the font-rescale shortcut with a
  dedicated GPU-data reload so same-size context transfers do not discard
  terminal image placements.
- macOS command: `make -C libkitty test` passed engine lifecycle, session API,
  render smoke, and `test_render_multi_context`, including A → B → A context
  transfer, geometry changes, a font-atlas rebuild, surviving-context render,
  and clean GL state. The clean macOS `HEAD` was tagged
  `macos-linux-port-baseline-2026-08-02-v0.21` at
  `3088295003c0842d7c3198102d0d05378da4dc62`.
- Linux source/cross-host command: `kitmux-linux/scripts/test-phase3.sh`
  passed 52 Rust tests, 6 contracts/20 cases/7 byte-identical fixture files,
  97 Linux inventory references, 234 macOS references, and all 9 macOS
  portable-contract consumer tests.
- Guarded command:
  `kitmux-linux/scripts/report-reference-drift.py --relock
  macos-linux-port-baseline-2026-08-02-v0.21` changed only
  `source-lock.json`, verified 7 locked reference files and the Linux overlay,
  then passed all 6 Ubuntu ARM64 C/C++/ELF/engine/session/stress tests and the
  same 52 Rust tests. Commit `90aa479` contains only that lock update.
- Source-tested: yes on macOS and Ubuntu ARM64, including cross-host fixtures.
  GUI-tested and release-layout-tested: not rerun for this source-only
  rebaseline; the recorded Phase 5 X11/native-Wayland and fresh-runtime gates
  remain the current product evidence. Native-package-tested,
  clean-desktop-installed, x86_64-runtime-tested, physical-input-tested, and
  physical-GPU-tested: none. Slice 6.1 is next; no Phase 6 product work was
  started.

### 2026-08-01 — Slice 5.3 and Phase 5 exit matrix complete

- Added a native GTK command palette over the frozen 38-ID catalog. Empty
  search preserves catalog order, exact/prefix/segment/contains matching is
  deterministic, unsupported commands stay visible but disabled, and Enter or
  a native button routes supported commands through the same product command
  path as shortcuts.
- Added a native settings dialog for restore policy, sidebar visibility and
  width, foreground-close confirmation, and paste threshold. It uses the
  existing bounded settings document and private atomic writer, preserves
  unknown fields, applies live, and supports Escape, Alt+C, and Ctrl+Enter.
- The state projection now saves and restores every workspace/group/tab,
  nested split root and ratio, schema-supported stable ID, name/title,
  selection, terminal surface stack, and safe per-surface cwd. Restore creates
  fresh passwd shells only, ignores browser surfaces, never executes saved
  resume text, and recovers the full hierarchy from last-good state after a
  corrupt primary.
- Pane, group, workspace, and whole-window close requests share one live
  foreground review path. Each scope is rechecked immediately before acting;
  the full-window path reviews every owned session, not only the selected one.
- Product controls expose native terminal/button roles and labels. The focused
  X11 and native-Wayland gates prove terminal → Commands → Settings → terminal
  focus transfer using GTK's accessible role/focus APIs.
- Source/cross-host command: `kitmux-linux/scripts/test-phase3.sh` passed 52
  Linux Rust model/contract/interaction/persistence tests, 6 contracts/20
  cases/7 byte-identical fixture files, and all 9 macOS portable-contract
  consumer tests. Final inventory validation resolves 97 Linux and 234 macOS
  references. SwiftPM used a temporary scratch path, leaving the macOS
  checkout read-only. A parallel-test temp-directory collision found on the
  first run was fixed with a process-local atomic sequence.
- Product command: `DISPLAY=:1
  kitmux-linux/scripts/test-phase5-product.sh` passed from a fresh 894-file
  release runtime. It proved mouse-free palette/settings use, scoped
  pane/group/workspace/window foreground review, a two-workspace/five-session
  nested hierarchy, byte-identical restart state, corrupt-primary last-good
  recovery, accessible roles/focus, and five-child reaping.
- X11 rapid-navigation command: `DISPLAY=:1 KITMUX_RAPID_NAV_GATE=1
  kitmux-linux/scripts/test-phase5-navigation.sh` passed 196 observed state
  transitions across nine tabs and nine workspaces, 17 permanent sessions,
  and complete child reaping. The equivalent native-Wayland command passed via
  `KITMUX_AUTONAVIGATION=1 KITMUX_WAYLAND_GATE=test-phase5-navigation.sh
  kitmux-linux/scripts/test-phase4-wayland.sh`.
- Native-Wayland split/accessibility command: `DISPLAY=:1
  KITMUX_SPLIT_GATE=1 KITMUX_ACCESSIBILITY_GATE=1 KITMUX_AUTONAVIGATION=1
  KITMUX_WAYLAND_GATE=test-phase5-navigation.sh
  kitmux-linux/scripts/test-phase4-wayland.sh` passed nested split/session
  ownership and native role/focus checks.
- Final regressions: `DISPLAY=:1 kitmux-linux/scripts/test-phase4.sh` and
  `DISPLAY=:1 kitmux-linux/scripts/test-phase4-wayland.sh` both passed after
  the completed Phase 5 UI. The Wayland run caught and fixed an 874 px natural
  minimum-width error by stacking native app controls responsively; the gate
  now derives terminal input coordinates from the measured viewport.
- The phase-boundary `report-reference-drift.py --patch` report found clean
  macOS `HEAD` `29d4b7664c3684b791ec693969e10ed4629fc810` with a new additive
  `kitty_session_release_render_resources` API and related multi-context work.
  This is mandatory rebaseline drift. Re-locking was intentionally not run:
  its guard requires a clean Linux tree, while this checkout contains the
  active uncommitted port. The locked Slice 5 baseline still materializes and
  all Phase 5 gates above pass; a clean rebaseline is the Phase 6 precondition.
- Source-tested: yes. GUI-tested: Ubuntu 26.04 ARM64 X11 and native Wayland with
  llvmpipe. Release-layout-tested: yes through fresh 894-file runtimes.
  Cross-host-tested: yes. Native-package-tested, clean-desktop-installed,
  x86_64-runtime-tested, physical-input-tested, and physical-GPU-tested: none.
  Slice 5.3 and Phase 5 are closed; no Phase 6 product work was started.

### 2026-08-01 — Slice 5.2 splits and session ownership complete

- Replaced the single-session product shell assumption with a registry keyed
  by stable `SurfaceId`. Every created workspace, group, tab, or split terminal
  owns a real libkitty child and GLib PTY source until its model surface closes.
  Window teardown removes every source, closes every session, and verifies all
  direct children were reaped.
- One GTK `GLArea` renders the active tab's nested split layout through a
  bounded multi-region C bridge. Each visible surface receives its own
  scissored viewport and resize; inactive tabs/workspaces remain live and pump
  without entering layout or draw. Ordinary key, IME, mouse, selection, search,
  paste, and scroll paths dereference only the model-selected surface.
- Added pointer pane focus, a tolerance-band divider hit target, ratio-clamped
  divider drag, cycle/directional focus, and Super-based keyboard resize. The
  display-free model test `tab_resize_helpers_apply_keyboard_steps_and_pointer_ratios`
  covers both resize entry points and minimum-aware clamping.
- Ubuntu ARM64 source command: `limactl shell kitmux-linux-desktop -- bash -lc
  'cd "/Users/ethanabbate/Desktop/System/home-kitmux/operating-system/linux"
  && kitmux-linux/scripts/test-model.sh'` passed 51 Rust tests with formatting,
  warnings-denied Clippy, contracts, interaction, model, and persistence checks.
- X11 split command: `DISPLAY=:1 KITMUX_SPLIT_GATE=1
  kitmux-linux/scripts/test-phase5-navigation.sh` passed from a fresh 894-file
  release runtime. It created a three-pane nested tree backed by three children,
  rendered three regions, dragged a real divider using live-derived GTK
  coordinates, changed pointer focus, resized by keyboard, exited through the
  active terminal, and reaped all three children.
- Native-Wayland split command: `DISPLAY=:1 KITMUX_SPLIT_GATE=1
  KITMUX_AUTONAVIGATION=1 KITMUX_WAYLAND_GATE=test-phase5-navigation.sh
  KITMUX_WAYLAND_LABEL="Phase 5 native-Wayland split gate"
  kitmux-linux/scripts/test-phase4-wayland.sh` passed the same model/session,
  region-render, keyboard-resize, terminal-exit, and reap checks on
  `GdkWaylandDisplay`.
- X11 and native-Wayland hidden-session variants passed with
  `KITMUX_HIDDEN_SESSION_GATE=1`. A non-selected tab continued draining a
  bounded output loop through its own PTY source while the selected terminal
  remained the only input target; both children were reaped.
- Source-tested: yes. GUI-tested: Ubuntu 26.04 ARM64 X11 and native Wayland with
  llvmpipe. Release-layout-tested: yes through fresh 894-file runtimes.
  Native-package-tested and clean-desktop-installed: none. Slice 5.2 is closed;
  Slice 5.3 product controls and persistence are next.

### 2026-08-01 — Slice 5.1 navigation hierarchy complete

- Added bounded workspace/group/tab naming to the existing Rust hierarchy,
  explicit tab/group/workspace close operations that cannot empty their
  parent, and workspace cycling. Reorder and close continue to preserve stable
  IDs and close removed abstract runtimes exactly once.
- Extended the existing stable-command-ID shortcut map with navigation
  actions. Linux number namespaces use Super+1…9 for workspaces and Alt+1…9
  for terminal tabs; extra modifiers are rejected, and plain terminal Control
  chords remain unclaimed. Navigation overrides use the same settings path as
  the seven Phase 4 terminal actions.
- The frozen reference still materializes exactly:
  `kitmux-linux/scripts/materialize-reference.sh` verified seven reference
  files and the one locked Linux overlay.
- Cross-host source gate: `kitmux-linux/scripts/test-phase3.sh` passed 50 Rust
  model/contract/interaction/persistence tests, 6 contracts/20 cases/7
  byte-identical fixture files, 80 resolving Linux inventory references, and
  all 9 macOS portable-contract consumer tests.
- Ubuntu ARM64 source gate: `limactl shell kitmux-linux -- env
  CARGO_NET_OFFLINE=true <mounted-path>/kitmux-linux/scripts/test-model.sh`
  passed the same 50 Rust tests offline with warnings denied.
- Product integration compile: a fresh temporary CMake Release build with
  `KITMUX_BUILD_APP=ON`, the pinned Python root, and isolated libpython built
  `kitmux_app` successfully against GTK 4.22.4 in the Ubuntu ARM64 desktop VM.
- Wired the model into a responsive GTK workspace sidebar, group row, and tab
  row. Native controls create, select, rename, reorder, and close hierarchy
  nodes; the same stable command IDs drive product navigation shortcuts. The
  projection keeps weak widget references so window teardown cannot form a
  GTK ownership cycle.
- Added `kitmux-linux/scripts/test-phase5-navigation.sh`. Its fresh release
  runtime passed real X11 key routing for workspace/group/tab creation and
  selection, then passed the same state sequence on `GdkWaylandDisplay` through
  the shared nested-Weston harness. The Wayland path exits through the live
  terminal after navigation, proving the original shell remains usable.
- Re-ran `kitmux-linux/scripts/test-phase4.sh` and
  `kitmux-linux/scripts/test-phase4-wayland.sh`. Both full terminal regressions
  pass after the new layout; the Wayland gate also caught and closed a 1064 px
  minimum-width regression by stacking group and tab controls responsively.
- Source-tested: yes. GUI-tested: Ubuntu 26.04 ARM64 X11 and native Wayland with
  llvmpipe. Release-layout-tested: yes through fresh 894-file runtimes.
  Native-package-tested and clean-desktop-installed: none. Slice 5.1 is closed;
  Slice 5.2 session multiplication and nested splits are next.

### 2026-07-28 — Phase 4 exit matrix

- Source-tested: all 48 Rust model/contract/interaction/persistence tests,
  six Linux headless C/ELF/session tests, locked warnings-denied model/app
  Clippy, formatting, portable fixture validation, and the nine-test macOS
  compatibility consumer pass. The inventory resolves 75 Linux and 234 macOS
  references across 64 features.
- GUI-tested: the final X11 and nested native-Wayland product gates pass from
  fresh 894-file release runtimes. Both prove configured shortcut routing,
  external clipboard copy/paste during PTY output, selection, paste safety,
  search, mouse/wheel, resize, and child reaping; X11 also proves the
  foreground-close dialog. A direct X11 focus call removed an intermittent
  nested-Wayland input-handoff race in the gate.
- Program-tested: `limactl shell kitmux-linux-desktop -- env DISPLAY=:1
  "$PWD/kitmux-linux/scripts/test-phase4-programs.sh"` passed explicit Bash,
  Zsh, Fish, Vim, Less, and tmux input/return paths.
- Soak-tested: `limactl shell kitmux-linux-desktop -- env DISPLAY=:1
  "$PWD/kitmux-linux/scripts/test-phase4-soak.sh"` passed for 1,801 monotonic
  seconds with 1,090 resize/pointer/wheel/font cycles, 160 bounded shell
  heartbeats, and a 220 ms maximum heartbeat against the 5-second limit. The
  shell and continuous flood worker were reaped.
- Clean-runtime-tested: `kitmux-linux/scripts/test-phase4-clean-target.sh`
  passed in a runtime-only Ubuntu 26.04 Xvfb container with no compiler,
  Cargo, CMake, Make, pkg-config, or GTK/GLib/Epoxy development package. Its
  base is digest-pinned in `source-lock.json`, verified against the
  Containerfile, and fetched by exact digest when missing.
- The phase-boundary reference report still shows only the reviewed Phase 3
  portable-fixture consumer commit plus one relevant uncommitted macOS test;
  no libkitty/API change requires rebaselining, and the dirty macOS test keeps
  optional re-locking correctly blocked.
- Source-tested, GUI-tested, and clean-runtime-tested evidence is complete for
  Phase 4 on Ubuntu 26.04 ARM64/llvmpipe. Native-package-tested,
  clean-desktop-installed, x86_64, physical-input, and physical-GPU evidence
  remains future work. Phase 4 is closed; Slice 5.1 is next.

### 2026-07-28 — Slice 4.3 minimal crash-safe persistence

- Added one display-free persistence policy over the existing XDG resolver,
  bounded state/settings codecs, private atomic replace, and replacement-aware
  fingerprint watcher. Missing files use defaults; malformed and newer files
  are moved byte-for-byte through no-clobber same-directory set-asides; a
  failed set-aside disables overwrite for that launch. State writes preserve
  the previous primary as last-good before replacing the primary.
- Hardened the shared atomic writer so its app directory and any existing
  destination must belong to the effective user; the app directory is `0700`,
  files are `0600`, symlink/non-file targets are rejected, and failed writes
  remove their temporary file without changing the previous destination.
- The GTK app loads state/settings before session creation, restores only an
  existing accessible absolute cwd and bounded font size, seeds restored cwd
  before PTY output can arrive, retains stable
  one-terminal IDs, watches valid settings replacements, and saves cwd/font
  before engine shutdown. Saved resume text, argv, and PIDs have no route to
  session creation: every launch still uses the passwd shell with `-il`.
- Source gate: all 48 model/contract/interaction/persistence tests passed,
  including no-clobber set-aside collision and failed-last-good ordering;
  locked warnings-denied Clippy and formatting passed
  for the model and product app.
- Product gate: `limactl shell kitmux-linux-desktop -- env DISPLAY=:1
  "$PWD/kitmux-linux/scripts/test-phase4-persistence.sh"` passed. It built one
  fresh release runtime and proved missing defaults, private deterministic
  writes, stable IDs, cwd/font restoration into a different fresh shell PID,
  immediate-exit cwd retention, inert seeded resume text, pre-directory watcher
  arming, atomic replacement, removal/recreation, malformed-edit recovery,
  same-content suppression, unsafe-paste behavior after live settings reload,
  corrupt/newer byte-preserving no-clobber set-asides, blocked-quarantine
  overwrite refusal, and read-only preservation.
- The same gate mounted an isolated 64 KiB private tmpfs, filled it, observed a
  real state-save failure under `ENOSPC`, and proved the prior snapshot hash was
  unchanged with no `.kitmux-write-*` remainder. Every launched shell was
  reaped and the mount was removed.
- GUI-tested: Ubuntu 26.04 ARM64 X11/llvmpipe. Package-tested and clean-machine
  desktop-tested: none. The bounded watcher is polling-based by contract for
  this one-writer alpha; no inotify-specific claim is made.
- Slice 4.3 is closed. The Phase 4 exit matrix is next.

### 2026-07-28 — Slice 4.2 terminal interaction

- Added the display-free interaction policy for paste safety, exact
  scale-aware cell mapping, fractional scroll accumulation, Linux shortcut
  routing, and wrapped URL detection. Six focused interaction tests pass,
  including exact large-paste boundaries, control characters, half-cell
  mapping, invalid scale, smooth-scroll residue, plain Control-C ownership,
  configurable shortcut overrides, ambiguous chords, wrapped URLs, email
  links, and rejected schemes.
- Wired the production GTK app to the existing durable key translator and
  libkitty APIs: shortcuts run before IM filtering; clipboard reads are
  asynchronous; unsafe paste defaults to cancel; selection and mouse reporting
  honor Shift override; press/drag/release, wheel/smooth scrolling, search,
  font controls, Ctrl-click URL opening, and foreground-process close
  confirmation are live product paths. Linux-only settings override the seven
  implemented actions through stable command IDs and are reapplied on settings
  reload without extending the portable settings schema.
- X11 product gate: `limactl shell kitmux-linux-desktop -- env DISPLAY=:1
  "$PWD/kitmux-linux/scripts/test-phase4.sh"` passed from a freshly rebuilt
  894-file runtime. It proved plain Control-C reaches the terminal, external
  clipboard copy/paste, rejected and confirmed large paste, selection text,
  a configured non-default search shortcut, search/cancel, font controls,
  mouse press/drag/release, raw wheel delivery, PTY progress during pointer
  activity, and cancel-then-confirm close with a live foreground reader. The
  shell child was reaped.
- Native-Wayland product gate: `limactl shell kitmux-linux-desktop -- env
  DISPLAY=:1 "$PWD/kitmux-linux/scripts/test-phase4-wayland.sh"` passed under
  nested Weston without Xwayland and asserted `GdkWaylandDisplay`. The same
  production controller handled shell input, live resizes, external
  `wl-copy`/`wl-paste`, unsafe paste, selection, the configured non-default
  search shortcut plus navigation/cancel, font, mouse, wheel, pointer-time PTY
  progress, and child-exit/reaping. XTEST cannot issue a Wayland toplevel
  close, so the X11 run remains the close-dialog gate.
- The release runtime's generated `session_api` gate separately re-proved
  bracketed paste, exact scrollback/search/selection behavior and independent
  session interaction state. Warnings-denied Clippy, formatting, loader
  isolation, SBOM validation, and secret/path-leak checks are included in the
  same fresh runtime build.
- GUI-tested: Ubuntu 26.04 ARM64 X11 and native Wayland clients with Mesa
  llvmpipe. Package-tested and clean-machine desktop-tested: none. Physical
  libinput/touchpad, physical GPU, end-to-end default URL-handler launch, and
  native packaging remain unproven.
- Slice 4.2 is closed. Slice 4.3 minimal crash-safe persistence is next.

### 2026-07-28 — Slice 4.1 application shell and lifecycle

- Added the production `kitmux` Rust/GTK target without promoting the
  disposable spike host. It owns one GTK window, `GtkGLArea`, engine and
  libkitty session; resolves the passwd login shell and account home; pumps
  its PTY at idle priority; updates render, scale, viewport, title, and cwd;
  and reports secret-safe lifecycle diagnostics.
- The product target now builds independently of the spike-only GTK host,
  WebKitGTK, and XTest. Its release runtime isolates `libkitty` and pinned
  libpython under `lib/app`, keeping Kitty's private native dependency closure
  out of distro GTK's loader namespace. Kitty shell integration and terminfo
  are included for real interactive programs.
- Product GUI/artifact gate: `limactl shell kitmux-linux-desktop -- env
  DISPLAY=:1 "$PWD/kitmux-linux/scripts/test-phase4.sh"` built a fresh
  release-shaped runtime with 894 files and 24 SPDX packages, launched with a
  clean environment and no `LD_LIBRARY_PATH`, proved the terminal child was
  the passwd shell, exercised title/cwd updates, 50,000 lines of output and
  repeated live resizes, then exited and proved the child was gone. Loader
  audits found no developer path or private-library leakage.
- Source gate: warnings-denied locked Clippy passed for every product-app
  target in the desktop VM. `rustfmt` and the app lock hash are part of the
  checked input set.
- ADR 0008 R3 also closed in a current full desktop run. With caller-supplied
  `DISPLAY=:1` and no noVNC requirement, the gate passed X11 and native
  Wayland render/input/IME/WebKit paths, virtual scaling, relocated loading,
  accessibility, and five-session fairness. The 12-second fairness run
  recorded 550 heartbeats, 714 frames, all 60 resizes, 62.322 ms maximum
  heartbeat, 25.116 ms maximum frame latency, and at least 118,268,073 bytes
  per session. The gate restores the complete X repeat/rate, XKB, and active
  IBus state it changes.
- GUI-tested: Ubuntu 26.04 ARM64 X11/llvmpipe product shell. Package-tested
  and clean-machine desktop-tested: none. The runtime is an unpackaged
  release tree; native Wayland product interaction and the full shell/editor
  matrix remain Phase 4 exit work.
- Slice 4.1 and R3 are closed. Slice 4.2 terminal interaction is next.

### 2026-07-26 — Slice 0.6 macOS reference-drift ritual

- Added `kitmux-linux/scripts/report-reference-drift.py`. Its default Markdown
  report compares the tag and commit in `source-lock.json` with current macOS
  `HEAD`, restricted to `libkitty/`, `patches/`, and the reusable contract and
  behavior paths listed in `LINUX_PORT_PLAN.md`. It classifies every committed
  file and lists relevant uncommitted files separately so working-tree edits
  cannot be mistaken for the `HEAD` comparison.
- The first report measured nine committed contract-affecting files between
  `e39381a` and `fb295fb`: the intentional portable-fixture mirror and its
  macOS consumer tests from Slice 0.4. There was no public libkitty-header,
  patch, libkitty implementation, or macOS behavior-source drift. One
  uncommitted cross-host test edit was reported but excluded from the
  comparison.
- Rebaselining was not required: the committed drift adds consumers of the
  already frozen corpus without changing the frozen engine or product
  behavior target. The existing tag and lock remain unchanged. The current
  dirty macOS test edit would block rebaselining until committed or removed.
- `--patch` appends the complete restricted patch. The guarded
  `--relock NEW_TAG` mode requires clean macOS and Linux worktrees, requires
  the tag to point at current macOS `HEAD`, recomputes the locked reference
  hashes, materializes them, runs the Ubuntu headless gate, and refuses a
  result that changes anything except `source-lock.json`.
- Removed the duplicated tag and commit literals from
  `materialize-reference.sh`; materialization now reads the single
  authoritative values from `source-lock.json`.
- Static/source verification passed:
  `python3 -m py_compile
  kitmux-linux/scripts/report-reference-drift.py`;
  `kitmux-linux/scripts/report-reference-drift.py --self-test`;
  the live report; Bash and Zsh syntax checks; and
  `kitmux-linux/scripts/materialize-reference.sh`, which verified seven locked
  reference files plus one Linux overlay.
- Ubuntu ARM64 source gate:
  `limactl shell kitmux-linux --
  "$PWD/kitmux-linux/scripts/test-headless.sh"` passed six C/C++/ELF/engine/
  session/stress tests, the Rust/C header check, 6 contracts/20 cases/7 JSON
  files, inventory structure, and all 37 model/contract/import tests. The
  inventory's macOS-reference resolution was skipped inside the VM because
  the adjacent macOS checkout is not mounted there; the host-side drift report
  resolved the live repository and exact commits directly.
- GUI-tested, package-tested, and clean-machine-tested: none; Slice 0.6 changes
  reference reporting and re-lock workflow, not product behavior or artifacts.
- Slice 0.6 and Phase 0 are closed. Run the report at every later phase
  boundary and record the result even when no relevant drift exists.

### 2026-07-26 — Slice 2.3 GTK toolkit decision

- Selected GTK 4 after every Phase 2 gate passed. No written decision
  criterion failed, so the conditional Qt 6 probe was not warranted.
- Release-shaped GUI loader gate: `kitmux-linux/scripts/test-desktop.sh`
  installed a fresh temporary layout containing `bin/kitmux_gtk_host`,
  `lib/libkitty.so`, and the pinned `lib/libpython3.14.so.1.0`. The host used
  `$ORIGIN/../lib`, `libkitty` used `$ORIGIN`, `ldd` resolved both bundled
  libraries inside that layout, distro GTK/WebKit libraries stayed outside
  it, and no broad `LD_LIBRARY_PATH` was present. The relocated host rendered
  a live X11 frame and reaped its terminal child.
- Accessibility viability gate: the selected terminal surface exposed GTK's
  terminal role and `Terminal` label, reported focusable/focused state,
  transferred focus terminal-entry-terminal, and retained its
  `GtkIMMulticontext`. The same complete desktop gate re-proved the actual
  Compose and IBus preedit/commit paths.
- GUI-tested: `limactl shell kitmux-linux-desktop --
  "$PWD/kitmux-linux/scripts/test-desktop.sh"` passed the relocated loader and
  accessibility probe plus all prior X11, keyboard, IME, native Wayland,
  WebKitGTK, scaling, and fairness gates. The final fairness run recorded 557
  heartbeats, 716 frames, all 60 resizes, 48.220 ms maximum heartbeat, 21.632
  ms maximum frame latency, at least 118,929,825 bytes per session, and 3.541
  ms maximum single-pump time over 12 seconds.
- Source-tested: `python3 contracts/validate-fixtures.py` passed 6 contracts,
  20 cases, and 7 versioned JSON files; `python3
  contracts/validate-inventory.py ../macos/kitmux` resolved 40 Linux and 216
  macOS references; `limactl shell kitmux-linux --
  "$PWD/kitmux-linux/scripts/test-headless.sh"` passed all 6 C/C++/ELF/engine/
  session/stress tests, Rust/C header layout, fixture/inventory validation,
  and all 37 current Rust contract/import/platform/model tests.
- Package-tested and clean-machine desktop-tested: none. The temporary
  installed tree is loader-layout evidence, not a native package. The result
  is ARM64/X11/llvmpipe development-VM evidence. Physical GPU and mixed-monitor
  hardware remain Phase 6; complete AT-SPI screen-reader and terminal-content
  coverage remains Phase 5.
- Slice 2.3 and Phase 2 are closed. Slice 0.6 has since closed; Phase 4.1 is
  next.

### 2026-07-26 — Slice 2.2F GTK event fairness

- Added a bounded disposable GTK harness mode with one visible and four hidden
  libkitty sessions, each driven by a continuously readable PTY. The visible
  session records frame-request latency; a 20 ms GLib heartbeat records main-
  loop gaps; the desktop gate repeatedly resizes the live window 60 times.
- The first run disproved the existing `G_PRIORITY_DEFAULT` PTY source
  priority: default-priority heartbeats continued, but GTK rendered zero
  frames and applied zero resizes during the 12-second flood.
- Moving only the PTY fd sources to `G_PRIORITY_DEFAULT_IDLE` fixed the root
  cause. The passing 12-second X11/llvmpipe run recorded 550 heartbeats, 715
  frames, all 60 resizes, a 46.015 ms maximum heartbeat gap, a 24.779 ms
  maximum frame latency, and at least 107,653,266 bytes pumped by every
  session. The slowest individual pump was 2.149 ms.
- The gate bounds heartbeat gaps to 250 ms, frame latency to 500 ms, and an
  individual pump to 500 ms; it also requires at least 100 heartbeats, 20
  frames, 20 resizes, and 1 MB from every session. These are fairness bounds,
  not llvmpipe or physical-GPU performance claims.
- GUI-tested: `limactl shell kitmux-linux-desktop --
  "$PWD/kitmux-linux/scripts/test-desktop.sh"` passed every prior X11,
  keyboard, Compose/layout, IBus, native Wayland, WebKitGTK, and scaling gate
  before the new fairness assertions passed. Visible evidence is
  `kitmux-linux/gtk-fairness-proof.png`.
- Source-tested: the pre-change baseline `limactl shell kitmux-linux --
  "$PWD/kitmux-linux/scripts/test-headless.sh"` passed six C/C++/ELF/engine/
  session/stress tests, the Rust/C header gate, portable-contract validators,
  and all 33 then-current model/contract tests. The Slice 2.2F edit affects
  only the disposable GTK host and desktop scripts; its post-change build uses
  warnings as errors inside the desktop gate.
- Package-tested and clean-machine desktop-tested: none. The result is ARM64,
  X11, and Mesa llvmpipe in the dedicated VNC development VM. It proves GTK
  event scheduling under a deliberately saturated local workload, not GPU,
  compositor, x86_64, package, or clean-install performance.
- A VM restart also exposed a stale TigerVNC record and a user-systemd cache
  that had not loaded the portal units. The stale record was removed, and the
  existing desktop launcher now performs `systemctl --user daemon-reload`
  before restarting those units.
- Slice 2.2F is closed. Every Phase 2 kill test is green; Slice 2.3 has since
  passed and selected GTK 4.

### 2026-07-26 — Slice 3.3 cross-host compatibility and safe import preview

- Added the remaining display-free Linux consumer for the portable SSH
  profile/review fixture. It bounds and validates profile documents, rejects
  newer versions, duplicate IDs/names, invalid aliases, timestamps, and
  control-bearing commands, preserves unknown fields, parses only recorded
  `ssh -G` text, and reproduces the macOS review fingerprint and approval
  result. It has no process, shell, socket, or network path.
- Added `kitmux-import-preview`, a read-only macOS-state inspection command.
  It reports accepted structure and portable values, schema repairs,
  rejected/unsupported values, existing-path checks, `/Users/<name>`
  to an explicitly supplied existing Linux home translation, and every valid
  resume command as inert text requiring explicit approval. Newer schemas are
  reported without rewriting; unsafe/missing paths, invalid URLs, SSH profile
  references, invalid commands, oversized files, and symlink inputs are
  rejected. The preview has no write or execution API.
- Added a temporary Linux-produced compatibility bundle and a macOS consumer
  assertion for state, settings, split order, all 38 command IDs, control
  requests, SSH documents, and SSH review data. The bundle is generated in a
  temporary directory by `kitmux-linux/scripts/test-phase3.sh`; the canonical
  corpus and its byte-identical macOS mirror remain unchanged.
- Cross-host source gate: `kitmux-linux/scripts/test-phase3.sh` passed the
  6-contract/20-case/7-file mirror validator, inventory validation, formatting,
  warnings-denied Clippy, 22 contract/import/platform tests, 15 pure-model
  tests, and 9 macOS `PortableContractFixtureTests`, including direct macOS
  consumption of Linux-produced values.
- Ubuntu ARM64 source gate:
  `limactl shell kitmux-linux -- env CARGO_NET_OFFLINE=true
  "$PWD/kitmux-linux/scripts/test-model.sh"` passed the same Rust formatting,
  warnings-denied lint, 22 contract/import/platform tests, and 15 model tests
  with locked dependencies offline.
- The import safety test hashes the source before and after preview, includes
  command text that would create a marker if executed, and proves the source
  hash is unchanged and the marker is absent. A separate newer-version case
  is also byte-unchanged.
- GUI-tested: none; Slice 3 is intentionally display-free.
- Package-tested and clean-machine-tested: none. The macOS result used the
  existing development checkout and the Ubuntu result used the existing
  headless development VM. No live state import, state write, shell restore,
  SSH launch, SSH store, CLI installation, or package behavior is claimed.
- Slice 3.3 and Phase 3 are closed. Phases 0 and 2 have since closed; Slice 4.1
  is the next repository slice.

### 2026-07-26 — Slice 3.1 feature-inventory backfill

- Backfilled Slice 3.1's inventory obligation for
  `hierarchy.ownership`, `hierarchy.non-empty-invariant`,
  `hierarchy.focus-successor`, `hierarchy.reorder-and-rename`,
  `hierarchy.hidden-vs-visible`, `splits.tree-shape`,
  `splits.layout-arithmetic`, `splits.ratio-constraints`, and
  `splits.directional-focus`; each row now names its existing model tests and
  separates proven Linux behavior from the cross-host work still assigned to
  Slice 3.3. `splits.divider-interaction` remains unproven and unchanged.
- Changed `engine.fair-pumping` from bare `unproven` to `unproven; Slice
  2.2F`, matching the event-fairness gate that exercises several hidden
  sessions under load.
- Verification: `python3 contracts/validate-fixtures.py` passed 6 contracts,
  20 cases, and 7 versioned JSON files; `python3
  contracts/validate-inventory.py ../macos/kitmux` resolved 31 Linux test
  references and 214 macOS references and reported `inventory OK`;
  `kitmux-linux/scripts/test-model.sh` passed formatting, warnings-denied
  Clippy, 18 contract/platform tests, and all 15 existing model tests.
- This produced no new behavioral, cross-host, GUI, package, or clean-machine
  evidence; it only recorded existing Slice 3.1 and Slice 3.2 evidence. Slice
  2.2F event fairness remains the next slice.

### 2026-07-25 — Slice 2.2E fractional and mixed-output scaling

- Added a deterministic native-Wayland scaling gate under nested Sway 1.11
  with three X11-backed outputs configured at 100%, 150%, and 200%. The
  compositor advertises `wp_fractional_scale_manager_v1` and `wp_viewporter`;
  the test takes its display and runtime paths from the environment.
- GTK exposes two distinct quantities that cannot be collapsed:
  `gdk_surface_get_scale()` reported the compositor mapping `1.0`, `1.5`, and
  `2.0`, while `GtkGLArea` used integer backing-buffer factors `1`, `2`, and
  `2`. Kitty must build its font atlas against the latter; the compositor
  downsamples that two-times buffer for the 150% output.
- Added a narrow, hash-locked Linux overlay on the authoritative tagged
  libkitty source. `kitty_render_set_scale` rebuilds shared font data at the
  new backing scale while preserving point size and live sessions;
  `kitty_render_scale` reports the current backing scale. The overlay SHA-256
  is recorded in `source-lock.json`, and materialization verifies and applies
  it after extracting the tagged source rather than maintaining a duplicate
  libkitty tree.
- With a fixed 480x270 logical terminal area, exact samples were:
  100% — framebuffer 480x270, cell 14x27, grid 34x10;
  150% — framebuffer 960x540, cell 27x54, logical cell 13.5x27, grid 35x10;
  200% — framebuffer 960x540, cell 27x54, device cell 27x54, grid 35x10.
  Moving the same window back to 100% restored the original framebuffer,
  cell, and grid metrics exactly.
- All four samples contained `recorder-ready` and one unchanged child PID.
  The renderer rebuilt `1 -> 2 -> 1`, retained the 17-point font size, and
  clean close reaped the child. This proves the live terminal session survived
  unlike output scales without atlas or metric drift.
- GUI-tested:
  `limactl shell kitmux-linux-desktop --
  "$PWD/kitmux-linux/scripts/test-desktop.sh"` — every earlier X11, keyboard,
  Compose/layout, IBus, native Wayland, and WebKitGTK coexistence gate remained
  green before the new scaling gate passed. Visually inspected evidence is
  `kitmux-linux/gtk-scale-100-proof.png`,
  `kitmux-linux/gtk-scale-150-proof.png`, and
  `kitmux-linux/gtk-scale-200-proof.png`.
- Source-tested:
  `limactl shell kitmux-linux --
  "$PWD/kitmux-linux/scripts/test-headless.sh"` — all six C/C++/ELF/engine/
  session/stress tests passed in 6.47 seconds plus the Rust/C header layout
  check. `kitmux-linux/scripts/test-model.sh` also passed all 33 display-free
  model and contract tests.
- Package-tested and clean-machine desktop-tested: none. This is ARM64,
  llvmpipe client rendering under a nested Sway/pixman compositor with virtual
  outputs. It does not prove physical mixed-DPI monitors, vendor GPUs,
  simultaneous windows on unlike scales, GNOME/KDE behavior, x86_64,
  performance, or a distributable GUI layout.
- Slice 2.2E is closed. Slice 2.2F event fairness is the final remaining
  Phase 2 kill test; GTK is still only a candidate until Slice 2.3 passes.

### 2026-07-25 — Slice 3.2 bounded contracts and Linux adapters

- Extended `kitmux-linux/rust/model` with bounded state and settings codecs.
  State accepts the frozen v1 fixture, repairs legacy navigation drift, rejects
  duplicate IDs and empty structure, normalizes safe pane detail, limits
  snapshots to 8 MiB, and treats resume commands as inert data bounded to
  2,048 UTF-8 bytes. Settings validates all 17 portable keys and their exact
  defaults/ranges, limits documents to 1 MiB, rejects newer versions, and
  preserves unknown keys across encode.
- Added the exact 38-ID command catalog with display-free semantic actions and
  the current 44-method control catalog. The control codec enforces protocol
  version 1, 64 KiB request and 512 KiB response bounds, 256-byte request IDs,
  128-byte methods, response success/error invariants, error codes, and
  newline/CRLF stream framing.
- Added Linux-facing XDG config/state/data/cache/runtime resolution,
  `sun_path`-bounded Unix-socket addresses with a private fallback, owner and
  mode validation for runtime parents, SHA-256 file fingerprints, bounded
  symlink-rejecting reads, 0600 same-directory atomic writes with fsync, and a
  polling watcher that detects create, in-place edit, rename-replace, and
  removal.
- The feature inventory records partial evidence precisely. Slice 3.2 does
  not claim an inotify implementation, settings quarantine policy, real
  socket bind/stale-socket replacement, `SO_PEERCRED`, CLI installation, or
  cross-host fixture exchange. Those product and compatibility gates remain
  in their assigned later slices.
- Direct Rust dependencies are pinned in `Cargo.lock`. `serde`, `serde_json`,
  `sha2`, and `uuid`, plus their locked transitive dependencies, use licences
  compatible with the GPL-3.0-only Linux host. The new lockfile SHA-256 is
  recorded in `source-lock.json`.
- macOS-host source gate: `kitmux-linux/scripts/test-model.sh` — formatting,
  Clippy with warnings denied, and 33 tests passed: 18 bounded-contract and
  platform tests plus the existing 15 pure-model tests.
- Portable-corpus gates:
  `python3 contracts/validate-fixtures.py` and
  `python3 contracts/validate-inventory.py
  /Users/ethanabbate/Desktop/System/home-kitmux/operating-system/macos/kitmux`
  — 20 cases across 7 versioned JSON files passed, and all 214 macOS source
  and test references resolved.
- Ubuntu ARM64 source gate:
  `limactl shell kitmux-linux -- env CARGO_NET_OFFLINE=true
  "$PWD/kitmux-linux/scripts/test-model.sh"` — the same format,
  warnings-denied lint, frozen-fixture, codec, platform, and model suite passed
  all 33 tests from the integrated checkout; locked dependencies had already
  been fetched before the offline rerun.
- GUI-tested: none; the slice has no GPU or display requirement.
- Package-tested and clean-machine-tested: none. The Ubuntu result used the
  existing headless development VM and proves the Linux source lane, not a
  package or fresh-system install.
- Slice 3.2 is closed. Slice 3.3 is the next Phase 3 slice and must remain
  bounded to cross-host fixture consumption plus a read-only, non-executing
  macOS-state import preview.

### 2026-07-25 — Slice 3.1 display-free Rust model

- Added `kitmux-linux/rust/model`, a GPL-3.0-only Rust crate with distinct
  workspace, group, tab, pane, surface, and split ID types. Pane and split IDs
  retain the frozen Swift fixture's `rawValue` UUID representation.
- Implemented a content-neutral split tree with duplicate-ID rejection,
  depth-first layout order, branch collapse on close, pane swaps, ratio
  clamping, minimum-size-aware pixel layout, resize targets, and directional
  focus. The frozen `split-tree.json` accept and reject cases now drive a Linux
  consumer test directly.
- Implemented workspace/group/tab/pane hierarchy selection, cycling, reorder
  with active-object preservation, cross-hierarchy pane focus, and the close
  chain from pane through tab, group, and workspace. Closing the last live
  hierarchy returns an explicit host-close requirement instead of silently
  destroying the model.
- Added terminal and browser runtime interfaces with mocks. Pane containers
  own ordered surface stacks; the active surface alone can render or receive
  input, while inactive surfaces and hidden tabs/workspaces remain owned.
  Every non-closed terminal mock pumps while hidden, and closing a leaf closes
  every runtime in its surface stack.
- The crate has no GTK, WebKit, libkitty, filesystem, shell, or network
  dependency. Direct third-party dependencies are `serde` and `uuid`
  (MIT/Apache-2.0); test-only JSON parsing uses `serde_json`
  (MIT/Apache-2.0). Exact versions and registry checksums are in `Cargo.lock`;
  the lockfile hash is also recorded in `source-lock.json`.
- macOS-host source gate: `kitmux-linux/scripts/test-model.sh` — formatting,
  Clippy with warnings denied, and 15 tests passed.
- Ubuntu ARM64 source gate:
  `limactl shell kitmux-linux -- "$PWD/kitmux-linux/scripts/test-model.sh"` —
  the same formatting, lint, frozen-fixture, model, close, and mocked-runtime
  suite passed all 15 tests under Ubuntu 26.04.
- GUI-tested: none; the slice is intentionally display-free.
- Package-tested and clean-machine-tested: none. The Ubuntu result used the
  existing headless development VM and proves the Linux source lane, not a
  package or fresh-system install.
- Slice 3.1 closed with Slice 3.2 bounded to state/settings decode, control
  framing and errors, command semantics, and Linux
  XDG/file-watch/hash/socket adapters; that follow-on is now closed in the
  evidence entry above.

### 2026-07-25 — Slice 2.2D WebKitGTK conflict probe

- Added WebKitGTK 6.0 only to the disposable GTK host and mapped one
  `WebKitWebView` beside the live `GtkGLArea`. It loaded static in-memory HTML;
  the probe has no network request, navigation UI, browser chrome, or product
  data-session policy.
- WebKitGTK 2.52.3 loaded the fixture under `GdkX11Display` and native
  `GdkWaylandDisplay` while the real libkitty recorder remained visible. Kitty
  restored the tracked host GL state after every draw, and WebKit reported no
  load failure or web-process termination.
- Focus moved from the terminal into WebKit and back under both backends. The
  child received `61`, received nothing while an injected `x` targeted the
  focused web view, then received `7a` after terminal focus returned. Including
  the close sentinel, both exact streams were `617a1b5b32347e`; both terminal
  children were reaped.
- The host directly needs `libwebkitgtk-6.0.so.4`. Its live ELF closure
  resolved 139 libraries with none missing: `libpython3.14.so.1.0` stayed in
  `build-gtk/python-runtime`, while WebKitGTK, JavaScriptCoreGTK, and GTK came
  from Ubuntu's architecture library directory. Kitty's private development
  dependency directory did not enter the process closure.
- Installing the Ubuntu WebKitGTK development input added 43 distro packages
  and 166 MB on this VM. The principal runtime objects observed were about
  93 MB for WebKitGTK and 30 MB for JavaScriptCoreGTK. This is dependency-cost
  evidence, not a packaged-application measurement.
- The desktop startup now publishes its VNC display to the systemd user
  session and starts the GTK desktop portal reproducibly. This avoids a
  20-second failed portal lookup per WebKit-linked process after a VM restart.
  The TigerVNC session check also accepts both `1` and `:1`, making repeated
  startup idempotent.
- GUI-tested: `kitmux-linux/scripts/test-desktop.sh` passed every existing X11,
  deterministic keyboard, Compose/layout, IBus, and native Wayland gate plus
  the two WebKit coexistence runs. Visible evidence is
  `kitmux-linux/gtk-webkit-x11-proof.png` and
  `kitmux-linux/gtk-webkit-wayland-proof.png`.
- Source-tested: `kitmux-linux/scripts/test-headless.sh` remained green: six
  tests passed in 6.75 seconds plus the Rust/C header layout check.
- Package-tested and clean-machine desktop-tested: none. The result is ARM64,
  Mesa llvmpipe, one VNC/XFCE VM, and one nested Weston compositor; it does not
  prove a physical GPU, another distro, x86_64, browser product behavior, or a
  distributable dependency layout.
- Slice 2.2D is closed. Slice 2.2E scaling is next; event fairness remains
  Slice 2.2F. GTK is still a candidate until the complete Slice 2.3 gate.

### 2026-07-25 — Slice 0.4 portable contract fixtures

- Added an authoritative `contracts/fixtures/v1/` corpus: one manifest and six
  versioned contract files with 20 named accept, repair, reject, and ignore
  cases. The files cover state snapshots/stable IDs, settings defaults and
  validation, split-tree collapse/close order, 38 stable command identifiers,
  control framing/limits, and portable SSH profile/review data.
- Every contract file declares bounds and malformed-input behavior. Accepted
  fixtures exclude user paths, installed fonts, display shortcut encodings,
  browser data, shell/agent commands, sockets, keychain references, and
  package paths. The one rejected SSH security case contains a newline-bearing
  command only to prove validation drops it.
- The canonical corpus lives in this Linux repository. The tagged macOS
  reference carries a temporary SwiftPM test-resource mirror. The validator
  byte-compares every declared JSON file and rejects an extra or missing file,
  so the mirror cannot drift silently before the monorepo removes it.
- Added eight `PortableContractFixtureTests`: consumers exercise the production
  snapshot, settings, split, command, control, and SSH validators; producers
  build native macOS values and compare them semantically to the fixtures.
  UUID hex case is normalized because Foundation emits uppercase while other
  hosts commonly emit lowercase. The SSH review path parses recorded text
  directly and never starts `ssh`.
- Source-tested:
  `contracts/validate-fixtures.py --mirror <macOS fixture directory>` —
  6 contracts, 20 cases, and 7 byte-identical JSON files passed.
- Traceability-tested:
  `contracts/validate-inventory.py <tagged macOS checkout>` — 214 macOS
  references resolved; 64 features across 17 areas passed.
- macOS focused test:
  `swift test --filter PortableContractFixtureTests` — 8 tests passed.
- macOS full unit test:
  `swift test` — 291 tests passed. The isolated worktree used the clean tagged
  checkout's existing `libkitty.dylib` through `LIBRARY_PATH` only because
  SwiftPM links the unchanged executable target while running `KitmuxCore`
  tests.
- A first attempt to rebuild `libkitty.dylib` inside the long temporary
  worktree failed before tests: the pinned Python runtime did not have enough
  Mach-O load-command space for that longer checkout path. Moving the worktree
  shorter avoided the length issue, but the interrupted repair had already
  left its ignored local runtime unusable. No runtime artifact from that
  attempt is evidence for this slice.
- GUI-tested: none; the fixture suite is display-free.
- Package-tested and clean-machine-tested: none.
- Phase 3 is no longer blocked by provisional fixtures. Its Linux consumer
  will be implemented against these unchanged expectations when Slice 3.1 is
  explicitly started.

### 2026-07-25 — Slice 2.2C native Wayland client path

- Extended the existing disposable GTK desktop harness rather than adding a
  second host. It starts Weston 14.0.2 with its GL renderer and kiosk shell,
  without Xwayland, then launches the existing GTK host with
  `GDK_BACKEND=wayland`. The host reports `GdkWaylandDisplay`, so the GTK,
  `GtkGLArea`, input, and IME paths are native Wayland even though Weston is
  nested visibly in the dedicated X11 VNC desktop.
- The Wayland run rendered the real libkitty recorder session with Mesa
  llvmpipe, reported exact host-framebuffer sizes `1024x650`, `760x450`, and
  `980x590`, and byte-compared the tracked host GL state after Kitty drawing.
  The recorder child exited on its sentinel and its PID was dead after the
  GTK window closed.
- One ordinary `a` press/release and a held `a` reached GTK as native Wayland
  key events. Every repeat encoded to `61`; the count remains timing-bounded,
  as on X11. The exact recorder stream also contained the IBus result and
  close sentinel: `61(61)+c3a10d1b5b32347e`.
- A real `m17n:t:latn-post` IBus flow produced the visible preedit `a`, updated
  it in place to `á`, committed `c3a1` through the direct-write route, ended
  preedit, and encoded the composition-ending Return exactly once as `0d`.
  The nested compositor needs an explicit `IBUS_ADDRESS` because it owns a
  separate `XDG_RUNTIME_DIR`.
- XTEST is not used against the Wayland client. The existing injector drives
  only Weston's outer X11 window; Weston translates those events into
  `wl_keyboard` input for the native client. This is deliberately recorded as
  display-bound compositor-bridge evidence, not physical libinput/evdev or a
  general Wayland injection API.
- Visible evidence is `kitmux-linux/gtk-wayland-proof.png`.
- Source-tested: `kitmux-linux/scripts/test-headless.sh` — six tests passed in
  6.44 seconds plus the Rust/C header layout check.
- GUI-tested: `kitmux-linux/scripts/test-desktop.sh` — all existing X11,
  keyboard, Compose/layout, and IBus proofs remained green, followed by the
  native Wayland render/resize/GL/input/IME/clean-close gate.
- Not proven by this slice: a physical Wayland desktop or libinput device,
  physical GPUs, fractional/mixed-monitor scaling, WebKitGTK coexistence,
  event fairness, CJK candidate windows, surrounding-text, accessibility,
  selection, clipboard/paste, mouse/wheel, or search.
- Package-tested: none.

### 2026-07-25 — plan audit, licence decision, and Phase 2 reorder

No implementation, build, or runtime claim was made. Reviewed the plan, this
ledger, `NEXT_STEPS.md`, ADRs 0001–0005, the support matrix, the contracts,
and the live Linux source — CMake, the GTK host, the key translation, both
gate scripts, the release tooling, and the source-lock boundary — against the
macOS reference.

Findings and the changes they produced:

- **Licensing was a Phase 8 checklist item and should have been a Phase 0
  architectural constraint.** Kitty is GPL-3.0-only; the host links it
  transitively, so distribution places the whole combined work under GPL-3.0.
  The only architecture that avoids that is an out-of-process engine, which is
  a Phase 4 process-model decision — so the deferral would have silently
  foreclosed the option it was deferring. Decided: the Linux host is
  GPL-3.0-only free software (ADR 0006). `LICENSE` added at the repository
  root. Phase 0 gained Slice 0.5.
- **Phase 2 had its risk ordering backwards.** Slices 2.2C–2.2E — selection,
  clipboard, mouse, search — were the most expensive remaining work and the
  least likely to disqualify GTK, since VTE ships all of it. Slice 2.3 held
  four cheap checks that plausibly could: Wayland, WebKitGTK coexistence,
  fractional scaling, and main-loop fairness. Reordered: 2.2C Wayland, 2.2D
  WebKit probe, 2.2E scaling, 2.2F fairness. The interaction work moved to
  Phase 4 Slice 4.2, to be written once in the chosen host language.
- **The "disposable" spike was already being inherited.** `NEXT_STEPS.md`
  instructed later sub-slices to build on ~1,400 lines of key-translation and
  harness C that two ADRs described as throwaway, and nobody had decided
  whether the Rust host would call or replace it. ADR 0007 splits the spike
  explicitly: the display-free translation and its fixtures are durable and
  stay C behind FFI; `gtk_terminal_host.c` is disposable.
- **The physical-GPU requirement was blocking the toolkit decision on hardware
  access.** llvmpipe proves correctness; a driver proves driver behavior.
  Moved to Phase 6 as a beta obligation.
- **The feature inventory was too coarse to make Phase 9's parity gate
  falsifiable** — 16 rows against a ~32,000-line macOS application, with
  `navigation.hierarchy` covering an entire subsystem. Decomposition is now
  required before Phase 5.
- **Reference drift was unmeasured.** The macOS baseline is frozen at a tag
  while macOS is a live daily driver. Phase 0 gained Slice 0.6, a re-baselining
  ritual to run at every phase boundary.
- **Four reproducibility defects were named and given due dates** in ADR 0008;
  they are listed under current blockers below. No CI was added: the
  repository has no remote, and the desktop gate should not be edited by
  anyone who cannot run it.

### 2026-07-25 — Slice 2.2B Compose, layouts, and input methods

- The host no longer synthesizes committed text from key symbols. A
  `GtkIMMulticontext` sees every press first; `kitmux_translate_gdk_key` takes
  the committed UTF-8 as an argument and never invents it. Releases stay off
  the input-method path: the kitty keyboard protocol requires them to reach
  the terminal, and VTE likewise filters presses only.
- Commit routing mirrors the macOS text-input contract. The first single-scalar
  commit of a key event, with no composition already in progress, is
  re-encoded through `kitty_session_encode_key` so protocol-aware applications
  see the same event vocabulary they see for every other key. A composition
  result, dead key, Compose sequence, or asynchronous engine commit is written
  to the child as UTF-8 instead. The host logs which of the two routes each
  commit took.
- Preedit start/change/end drive an overlay label positioned at libkitty's own
  cursor cell, and the same rectangle is handed to
  `gtk_im_context_set_cursor_location` so an engine's candidate window
  follows the text. Focus loss resets the context and clears the overlay.
- Fixed a correctness problem this slice introduced: a press the input method
  swallowed used to leave a lone release behind, which is malformed under the
  kitty keyboard protocol. The host now withholds a release only when its
  press never reached the terminal as a key event — a press answered by the
  key-encoder route still owes its release.
- Added `ibus-m17n` and a polkit rule to desktop provisioning. XFCE asks
  polkit to create a colord-managed device on every session start; in a
  headless VNC session nobody answers and the dialog floated over the windows
  the gate screenshots as evidence.
- GUI-tested on GTK 4.22.4, X11, Mesa llvmpipe, in two more windowed runs:
  - GTK's own context, with the child byte stream `c3a6 c3a9 c3bc 40` —
    Compose `Multi_key a e` produced a visible `"a"` preedit and committed
    `æ` off the encoder path; a US-international dead key produced a visible
    `"'"` preedit and committed `é`; German `ü` and AltGr+q encoded as
    ordinary keys carrying committed text, the AltGr level correctly reporting
    its base key `q` with no kitty modifier bit.
  - IBus, with the child byte stream `61 c3a1 0d f09f9a80` — the pass-through
    `xkb:us::eng` engine left encoding to the key path; `m17n:t:latn-post`
    produced a real preedit that updated in place from `a` to `á`; Return
    committed `á` off the encoder path and was then encoded once in its own
    right; an emoji reached the child as `f09f9a80`.
- The Slice 2.2A runs now pin `GTK_IM_MODULE=gtk-im-context-simple`. A session
  with `ibus-daemon` already running would otherwise route ordinary typing
  through IBus and change which path encodes it.
- Visible evidence is `kitmux-linux/gtk-preedit-proof.png`: the live IBus
  preedit rendered at the terminal's cursor cell, over the byte the terminal
  had already received, beside the ordinary GTK controls.
- Source-tested: `kitmux-linux/scripts/test-headless.sh` — six tests passed in
  6.32 seconds plus the Rust/C header layout check.
- GUI-tested: `kitmux-linux/scripts/test-desktop.sh` — the Slice 2.1 and 2.2A
  proofs stayed green alongside the two new input-method runs. The key matrix
  grew to 32 events per terminal state.
- Not proven by this slice: selection, clipboard, paste, mouse, wheel, search,
  Wayland, scaling, and physical GPUs. CJK and other conversion engines are
  untested; `m17n:t:latn-post` was chosen because its preedit and commit are
  deterministic, not because it exercises candidate windows.
- Package-tested: none.

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
- This gate is the Slice 1.2 headless-test evidence: engine lifecycle, child
  exit status, PTY resize, foreground-process detection, descendant reaping,
  and many-session floods. The release-shaped tree below is Slice 1.3.
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

### 2026-07-23 — Slices 0.2 and 0.3 backfilled

- Backfill entry written 2026-08-02. Slices 0.2 and 0.3 were completed during the
  original planning pass but never received their own dated ledger entry, so Phase 0's
  closure rested on artifacts rather than on recorded evidence. This entry records the
  artifacts; it produces no new behavioral, GUI, package, or clean-machine evidence.
- Slice 0.2 — parity inventory: `contracts/feature-inventory.json` exists with 64
  features across 17 areas. Every row carries a stable ID, behavior, macOS
  source/test references, a classification, a Linux acceptance statement,
  dependencies, translation notes, and a `linux_status`. Verified by
  `python3 contracts/validate-inventory.py`, which resolved 97 Linux test references
  and 234 macOS source and test references at
  `macos-linux-port-baseline-2026-08-02-v0.21` and reported `inventory OK` when this
  entry was written. The Linux count grows as slices add tests — Slice 6.1 took it to
  105. Run the validator for the current figure rather than quoting this one.
  The 2026-07-25 plan audit found the original 16 rows too coarse for Phase 9's parity
  gate; decomposition to per-behavior rows was completed before Phase 5.
- Slice 0.3 — decisions and support targets: ADRs 0001 through 0005 exist under
  `docs/decisions/` and cover the Rust host, the GTK kill spike, repository and
  engine source, contract versioning, and Python/packaging. `support-matrix.yml`
  exists and records OS images, architecture, desktop, display backend, GPU class,
  compiler, Python, toolkit, and package targets. ADRs 0006, 0007, and 0008 were
  added later by the 2026-07-25 plan audit.
- Source-tested: inventory and ADR/matrix structure only. GUI-tested,
  package-tested, and clean-machine-tested: none. No slice behavior was re-run to
  produce this entry.

### 2026-07-23 — planning review

- Inspected the original review, the revised plan that replaced it during this
  pass, the macOS handoff and Git status, package manifest, `libkitty`
  build/header/sources, platform imports, tests, and smoke targets.
- Added the agent guide and status ledger.
- No implementation, build, or runtime claim was made.

## Current blockers and limits

- The local desktop VM proves a real GTK/libkitty session over X11 and a
  native Wayland client path under nested Weston, both with llvmpipe. It
  includes press/release/repeat routing, focus transfer to an ordinary GTK
  control on X11, Compose, dead keys, AltGr, a non-US layout, and a real IBus
  preedit/commit flow on both display backends. It does not prove performance
  or driver behavior. The Phase 4 product proves selection, external
  clipboard/paste, mouse/wheel, and search on both display backends. A physical
  Wayland desktop/libinput path, physical touchpad, physical GPU, physical
  mixed-DPI monitor behavior, and complete AT-SPI content/screen-reader
  behavior remain untested. Fractional and unlike-output scaling passed under nested Sway with virtual outputs; it
  is correctness evidence, not physical hardware or performance evidence.
  Event fairness passed with five saturated PTYs, 60 live resizes, and bounded
  UI latency. The bounded WebKitGTK coexistence check passed, but browser
  product behavior remains unimplemented and out of scope for the terminal-first alpha.
- Input-method evidence covers one Latin conversion engine. CJK engines,
  candidate windows, and surrounding-text requests are unproven; IBus already
  warns that the host has no surrounding-text capability.
- The desktop gate changes session-wide state while it runs: X auto-repeat,
  the keyboard layout, and the active IBus engine. It restores auto-repeat and
  the complete prior XKB rules/model/layout/variant/options and IBus engine on
  exit. It is meant for the project VNC session, not a desktop in use.
- The GUI keyboard runs assert exact bytes per event but bound, rather than
  fix, the number of auto-repeats: X auto-repeat is the only way to produce a
  real GDK repeat event, and its count is timing-dependent. The fixed
  press/repeat/release expectations live in the display-free key matrix.
- The current VM is ARM64. Tier-1 x86_64 proof still requires CI, a remote
  machine, or a separate emulated/native environment.
- Shared portable fixtures are frozen and byte-identical in the current macOS
  mirror. Phase 3 consumes every contract family, macOS consumes the
  Linux-produced compatibility bundle, and the safe import preview is
  read-only. No live import or imported-command execution path exists; the
  separate Phase 4 product persistence writer stores only current safe cwd and
  font state.
- The clean release gates prove ARM64 build userspaces and a runtime-only
  Ubuntu target with no compiler, Cargo, CMake, Make, pkg-config, or GTK/GLib/
  Epoxy development package. Tier-1 x86_64, physical-GPU, remote-CI, signing,
  vulnerability, and complete desktop-menu evidence remain future gates.
- Fresh clean-gate runtimes were approximately 104 MiB. Ignored local build
  directories may contain older layouts and are not release evidence; always
  build to a new path and run the runtime audit.
- A temporary installed GUI tree proves the isolated-libpython loader boundary
  through relative runpaths without a broad `LD_LIBRARY_PATH`. It is not a
  native package or clean-machine install. A global Kitty dependency path
  would shadow GTK's distribution libraries and remains forbidden.

### Carried-forward Phase 4 review findings (historical snapshot)

A second-pair-of-eyes review on 2026-07-28 recorded eight findings against the
in-flight Slice 4.1/4.2 tree and none were fixed before Phase 5 closed. The full
review text is in commit `e88a91b` (`docs/PHASE4_REVIEW_NOTES.md`, since removed);
this ledger is the live record. Status as of 2026-08-02:

- **#1 environment-disableable safety prompts — closed.** `KITMUX_AUTOPASTE` and
  `KITMUX_AUTOCLOSE` are now behind the off-by-default `test-hooks` cargo feature.
- **#2 build.rs did not relink on C changes — closed.** `build.rs` now emits
  `cargo:rerun-if-changed` for both static archives.
- **#7 dangling event-source id — assessed unreachable.** The current close path
  removes the GLib source before deleting the corresponding session from
  `Terminal::sessions`; shutdown only visits remaining registry entries, so no stale
  ID can reach its loop under the current registry design.
- **#3 two exported bridge functions have no caller — open.**
  `kitmux_widget_surface_scale` and `kitmux_session_draw_preserving_gl_state` in
  `src/gtk_terminal_bridge.c`. The review verified this is not a scaling bug. Delete
  them or record why they are kept.
- **#4 `let _ = committed;` — open.** `rust/app/src/main.rs` in the key-pressed handler.
  Either the commit state should affect whether the release is withheld, or this is a
  leftover.
- **#5 shutdown runs and logs twice — open, mitigated.** `connect_close_request` and
  `connect_unrealize` both call `shutdown`. The Phase 5 gates assert
  `sessions=N reaped=true` with N greater than zero, and the second call emits
  `sessions=0`, so it cannot satisfy them. The masking risk the review described is
  gated against, not fixed in code.
- **#6 libkitty's error text is discarded on startup failure — open.**
  `initialize` fills a 1024-byte buffer and returns a stage name without reading it.
  `render.init-failure-visible` is also one of the inventory rows with no macOS test,
  so no gate on either platform pins this.
- **#8 wheel speed is an unnamed constant — open.** `-dy * cell_points * 5.0`; kitty's
  own default is three. It belongs in settings.

The same review recorded four smaller items. Verified against the tree on
2026-08-02:

- **Duplicated `repr(C)` type in `main.rs` — closed.** `RenderResult` now lives in
  `rust/app/src/ffi.rs`. The remaining `unsafe extern "C"` block in `main.rs`
  declares GLib entry points, not libkitty layout, so it carries no drift risk.
- **`connect_close_request` returns `Propagation::Stop` on a borrowed `RefCell` —
  open.** The window cannot be closed for that window of time. A hang-shaped
  failure, not a crash-shaped one.
- **CMake installs `libkitty.so` to both `lib/` and `lib/app/` — open.** The two
  `install(TARGETS kitty …)` calls are guarded by `KITMUX_BUILD_GTK_HOST` and
  `KITMUX_BUILD_APP` separately, so enabling both muddies the app-only loader
  isolation Slice 4.1 claims. Release builds enable only the app path.
- **`build.rs` panics without `KITMUX_NATIVE_LIB_DIR` — open, accepted.** `cargo
  build` alone cannot build the app. This is one more thing ADR 0008 R1 has to
  answer.

The review also verified these as correct. Do not "fix" them without a
regression to point at: GL state capture/restore coverage; withholding a key
release only when the press never reached the terminal as a key event; the
Ctrl+Shift shortcut defaults that keep terminal control codes reachable;
diagnostics that record byte counts and status codes but never clipboard
contents, titles, or paths; the PTY source at priority 200 matching the Slice
2.2F fairness result; and the `--locked`/`--remap-path-prefix` cargo invocation
with a hashed app `Cargo.lock`.

### 2026-08-18 — Small code-debt follow-up

- Findings #3 and #4 are closed in the current worktree: the unused bridge
  exports were removed, and the input-method key path now uses the encoded
  decision directly. Finding #6 is closed by carrying the bounded libkitty
  diagnostic text through startup failure. Finding #8 is closed by the bounded
  persisted Linux wheel-lines setting. The CMake dual-install item is closed
  by making the GTK-host and app install destinations mutually exclusive.
- Finding #5 is closed by the idempotent shutdown guard; the product gate still
  proves expected session reaping. The borrowed close-request path now queues
  one main-loop retry instead of permanently stopping the close event while a
  sibling callback holds the model borrow.
- The current ARM64 app Clippy gate and Phase 5 product gate passed after these
  changes. `build.rs` still requires `KITMUX_NATIVE_LIB_DIR`; that is an
  accepted build-contract limitation, not silently reclassified as fixed.

Six inventory rows carry no `macos_tests`, five of them terminal-alpha:
`render.gl-state-isolation`, `render.scale-correctness`,
`render.init-failure-visible`, `render.webkit-coexistence`, and
`keyboard.press-release-repeat`. Phase 9's parity gate resolves rows against macOS
behavior; these have no macOS oracle.

Slice 0.1 remains unmarked: its 2026-07-23 evidence names the baseline gates by family
but does not enumerate the seven required `make` commands and
`git status --short --branch`, so this ledger does not claim that gate was fully
evidenced.

### Reproducibility defects (ADR 0008)

These are properties of the tree, not of any one slice. R1, R2, and R5 are
being closed in the current release-readiness work; the exact passing evidence
is recorded only after the corresponding gate runs.

- **R1 — closed 2026-08-18.**
  `kitmux-linux/locked-inputs/reference` now carries the tagged libkitty and
  Kitty patch inputs, `materialize-reference.sh` uses it without touching
  `../macos/kitmux`, and `test-standalone.sh` passed in the ARM64 headless VM
  with that checkout unavailable. This closes the current ARM64 standalone
  source obligation; x86_64 remains unbuilt.
- **R2 — closed 2026-08-27.** `.github/workflows/linux-standalone.yml` passed
  on the GitHub-hosted `ubuntu-24.04-arm` runner: run 33131978184 at commit
  `3a4aa88` (<https://github.com/DigitalWestern/kitmux-linux/actions/runs/33131978184>),
  standalone-arm64 green end to end — locked materialization, kitty build,
  6/6 ctest including the ELF audit and session/engine suites, Rust header
  layout, portable fixtures, and the full model gate. The standalone-amd64
  job also passed using the explicitly unlocked x86_64 fallback; per ADR
  0008 that is fallback smoke evidence, not an x86_64 support or
  reproducibility claim. Getting here surfaced and fixed five clean-host
  defects the persistent VM state had masked (workflow dev packages and
  toolchain, missing symbols-font extraction, missing dependency-tree
  relocation, CMake linking the host libpython, and libkitty's runpath
  leaking the dependency path); the dated 2026-08-27 entries carry the
  detail.
- **R3 — closed 2026-07-28.** `scripts/test-desktop.sh` accepts a
  caller-supplied `DISPLAY`; noVNC is optional in that mode. Its full X11 and
  nested-Wayland gate passed after the change, and cleanup restores the
  original X repeat state and rate, complete XKB rules/model/layout/variant/
  options, and active IBus engine.
- **R4 — x86_64 inputs are unpinned. Closed 2026-07-28, ahead of its due date.**
  `source-lock.json` now records `linux-64.tar.xz` at
  `3d0ffc610b99d374245a557387d102abfc6f55e2478f9dfd596a11f1f6ce709d`, so
  `release-tools.py verify-inputs --platform linux-64` has a value to check
  instead of failing on a missing checksum. The hash came from
  `https://download.calibre-ebook.com/ci/kitty/linux-64.tar.xz`, which is the
  `BUNDLE_URL` in `.github/workflows/ci.py` at locked Kitty commit `c1d507d`
  resolved for `runtime.GOARCH == amd64` by `bypy/devenv.go`. The arm64 bundle
  was re-fetched from the same location in the same operation and hashed to the
  already-locked `eb54002…`, which is what establishes that both recorded
  values describe the same upstream build; both bundles carry a 2026-07-03
  `Last-Modified`. This is a recorded hash, not a passing gate: ADR 0008 is
  explicit that locking x86_64 inputs is not permission to claim x86_64
  support, and no x86_64 build has been attempted.
- **R5 — partial durable mirror; exact x86_64 input still open.** The ARM64 and
  Nerd Fonts bundles are mirrored under
  `kitmux-linux/locked-inputs/dependency-bundles` and match `source-lock.json`.
  On 2026-08-18 the rolling x86_64 URL returned digest
  `d1472771e9c976dfc9a65f7cfc625ec3ec86dab6d5d612393d339db4445924b7`, not the
  locked `3d0ffc610b99d374245a557387d102abfc6f55e2478f9dfd596a11f1f6ce709d`.
  That artifact was rejected and kept outside the tree; do not change the lock
  until the exact historical input is recovered or intentionally re-locked.

All evidence in this ledger was produced by one person, by hand, in two
Lima VMs on one Apple-silicon macOS machine. That is adequate for the current
phase and is not adequate for Phase 8 or for a second contributor.

## Next-agent handoff

Follow the physical-Mesa and reproducibility order in [`NEXT_STEPS.md`](NEXT_STEPS.md).
Phases 0 through 5 and Slices 6.1–6.3 are closed; GTK 4 is selected, and the
release-shaped terminal multiplexer alpha has hierarchy navigation, nested
splits, one permanent session per live surface, native command/settings
controls, full safe hierarchy persistence, reviewed SSH workflows, inert
resume/recovery, close-chain foreground review, and an in-window menu bar.
The macOS/libkitty v0.21 reference is locked and the ARM64 standalone/headless
gates pass. The app crate was modularized on 2026-08-27 with the desktop
navigation/control/resume/SSH/Phase 4 gates green; the one open app gate is
the deterministic `test-phase5-product.sh` F10 arrow/Escape menu-traversal
check (MENUBAR_PLAN slice 4).

Phase 3 is closed through Slice 3.3. Do not expand its preview into a live
state import, shell restore, or persistence writer. Do not begin browser
product functionality, native packaging, repository migration, or new SSH
feature work. Physical-Mesa GPU proof remains a separate Phase 6 beta
obligation.
