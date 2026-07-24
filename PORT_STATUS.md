# Kitmux Linux Port Status

**Last inspected:** 2026-07-24

**Implementation state:** Phase 1 is closed. Separate Ubuntu ARM64 headless and
XFCE desktop VMs, an ELF `libkitty.so`, a relocatable 104 MB attributed engine
runtime, Linux stress tests, and a visible GTK 4 `GtkGLArea` smoke exist. No
production GTK application or native package exists yet.

**Active phase:** Phase 2 — rendering and toolkit kill spike.

**Next slice:** Slice 2.1 — host one real libkitty terminal surface in
`GtkGLArea`, integrate its PTY with GLib, and prove visible render/resize/close
behavior over X11. Phase 0.4 shared fixture promotion remains open and blocks
the Rust product model, not the GTK kill spike.

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
- Rust plus GTK 4 is the first candidate, not a final choice. A disposable GL,
  input, scale, and event-loop spike decides the production stack.
- The first user-facing target is a terminal-only alpha. Browser panes and
  broad distribution support are later decisions.

## Evidence log

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
- GUI-tested: no new GUI claim belongs to Slice 1.3; the earlier clear-color
  GTK/X11 desktop smoke remains the current GUI evidence.
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

- The local desktop VM proves GTK/OpenGL window creation over X11 with
  llvmpipe. Wayland, physical-GPU behavior, IME, clipboard, scaling, and
  desktop packaging remain untested.
- The current VM is ARM64. Tier-1 x86_64 proof still requires CI, a remote
  machine, or a separate emulated/native environment.
- Shared portable fixtures are still provisional. Do not start the Rust model
  or claim snapshot/control parity until macOS and Linux consume the same
  valid and invalid fixture files.
- The clean release gate currently proves ARM64 userspaces. Tier-1 x86_64,
  physical-GPU, native package, and clean desktop-install evidence remain
  future gates.

## Next-agent handoff

Continue
[Slice 2.1 in the implementation plan](LINUX_PORT_PLAN.md#slice-21-host-one-real-terminal-surface).
Build the smallest real GTK/libkitty surface and prove it in the desktop VM.
Do not begin input breadth, product chrome, the Rust model, or browser work
until that slice's render/lifecycle gate is concrete.
