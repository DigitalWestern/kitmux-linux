# Kitmux

**Work in progress.** Neither the Linux port nor the macOS app is complete;
this repo in particular is an experimental port with no packaged download yet.

Kitmux is a native terminal workspace built around Kitty's terminal engine.
It organizes long-running terminal sessions into workspaces, groups, tabs,
split panes, and eventually mixed terminal/browser surfaces.

This repository contains the experimental Linux port. The complete macOS
application lives at
[DigitalWestern/kitmux](https://github.com/DigitalWestern/kitmux), and the
[Kitmux website](https://digitalwestern.github.io/kitmux-website/) covers how
the project was built. The two repos stay separate until the documented
monorepo migration happens in its own carefully reviewed session.

## Platform status

| Platform | Current state |
| --- | --- |
| macOS | Daily-driver application for macOS 13+ on Apple silicon. Terminal, navigation, persistence, settings, local control, SSH, browser panes, reliability gates, and local arm64 packaging exist. Public distribution still needs Developer ID signing, notarization, stapling, and real macOS 13 hardware qualification. |
| Linux | Experimental, GPL-3.0-only. Phases 0 through 5 and Slices 6.1–6.3 are complete: the engine passes headless and clean-runtime gates, GTK 4 is the selected toolkit, and a release-shaped terminal multiplexer alpha with secure local control, reviewed SSH workflows, and inert resume/recovery runs on X11 under Mesa llvmpipe. The ARM64 standalone source/headless gate and reproducible tarball/`.deb` lifecycle pass. x86_64, physical-GPU and mixed-DPI hardware behavior, complete AT-SPI coverage, browser product behavior, remote CI, and package promotion remain unproven. Exact per-slice evidence and non-claims are in [PORT_STATUS.md](PORT_STATUS.md). |

Linux is not yet a production application or promoted downloadable desktop
package; the current ARM64 artifacts are release-readiness evidence only.
GTK 4 was selected in Slice 2.3 on 2026-07-26 after every Phase 2 kill test passed;
see ADR 0002.

## Architecture

```text
platform host
  macOS: Swift + AppKit              Linux: GTK 4
                \                    /
                 public libkitty C API
                          |
              Kitty + embedded CPython
                          |
                       PTY child
```

The platforms should share observable contracts, fixtures, command IDs, and
one authoritative `libkitty` source. They should not share AppKit/GTK views,
platform lifecycle code, packaging, or signing machinery.

The Linux GTK process deliberately isolates only the pinned `libpython`.
Kitty's complete private development-library directory must not enter the GUI
loader path because it can shadow the distribution copies used by GTK, Cairo,
HarfBuzz, GLib, and xkbcommon.

## Build and test

For Linux setup, VM lifecycle, exact gates, runtime/SBOM generation, and known
limitations, read [Linux development](docs/LINUX_DEVELOPMENT.md).

The normal Linux gates, run from the macOS host at this repository root, are:

```sh
kitmux-linux/scripts/test-all.sh
```

For individual setup and diagnostic stages:

```sh
kitmux-linux/scripts/materialize-reference.sh
limactl start kitmux-linux
limactl shell kitmux-linux -- "$PWD/kitmux-linux/scripts/test-headless.sh"

limactl start kitmux-linux-desktop
limactl shell kitmux-linux-desktop -- \
  "$PWD/kitmux-linux/scripts/start-desktop.sh"
limactl shell kitmux-linux-desktop -- \
  "$PWD/kitmux-linux/scripts/test-desktop.sh"
```

The adjacent macOS source gate starts with:

```sh
make -C ../macos/kitmux/libkitty test
make -C ../macos/kitmux/macos/KitmuxApp smoke
```

The macOS handoff lists the additional packaged-app, persistence, browser,
control, SSH, reliability, and soak gates required for changes in those areas.

## Roadmap

Phase 2 was reordered on 2026-07-25 so the checks that could disqualify GTK
run before the expensive ones that almost certainly cannot.

1. Secure local control, SSH and agent workflows, and inert resume/recovery
   (Slices 6.1–6.3) are closed.
2. Prove one physical Mesa GPU before beta; llvmpipe proves correctness, not drivers.
3. Keep ADR 0008 R1 and the ARM64 dependency mirrors proven; run the R2 workflow
   on its configured remote runner.
4. Promote packages only after x86_64, hardware, CI, signing, and security gates
   close — current ARM64 tarball/`.deb` artifacts are test evidence.
5. Browser panes and desktop integrations are deferred for the terminal-only
   beta — Phase 7 is not approved.

The exact resume order is in [NEXT_STEPS.md](NEXT_STEPS.md). Proven evidence
and current blockers are in [PORT_STATUS.md](PORT_STATUS.md).

## Repository direction

The recommended destination is one private monorepo with separate platform
trees and root-level contracts/fixtures. The landing page should remain a
separate deployment repository. No repository migration has been performed.
See the [migration proposal](docs/MONOREPO_MIGRATION.md).

## Licensing

**The Linux host is GPL-3.0-only.** Original work in this repository is
Copyright (C) 2026 Ethan Abbate. Portions are derived from
[kitty](https://github.com/kovidgoyal/kitty), Copyright (C) 2016-2026 Kovid
Goyal, also GPL-3.0-only; Kitty itself is not redistributed here. See
[`LICENSE`](LICENSE) and
[ADR 0006](docs/decisions/0006-linux-license-posture.md).

Kitty is GPL-3.0-only, `libkitty` links it, and the host links `libkitty`, so
any distributed Linux artifact is a combined work under GPL-3.0. Distributing
one means shipping or offering complete corresponding source for the whole of
it — host, model, build scripts, and the exact Kitty and `libkitty` revisions
used. Public source release is therefore a prerequisite for public binary
distribution, not a parallel track.

The verified Linux engine runtime includes per-component notices and a
machine-readable SPDX 2.3 SBOM; its component manifest is
[`kitmux-linux/release/runtime-components.json`](kitmux-linux/release/runtime-components.json).
The SBOM proves the current engine payload inventory. It is not legal approval
for a future desktop package.

ADR 0006 governs Linux only. The same combined-work analysis plausibly applies
to any distributed macOS binary that links `libkitty`; nothing in this
repository is a legal opinion, and the owner should get one.
