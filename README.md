# Kitmux

Kitmux is a native terminal workspace built around Kitty's terminal engine.
It organizes long-running terminal sessions into workspaces, groups, tabs,
split panes, and eventually mixed terminal/browser surfaces.

This repository currently contains the experimental Linux port. The complete
macOS application remains in the adjacent `../macos/kitmux` repository until
the documented monorepo migration is performed in a separate, carefully
reviewed session.

## Platform status

| Platform | Current state |
| --- | --- |
| macOS | Daily-driver application for macOS 13+ on Apple silicon. Terminal, navigation, persistence, settings, local control, SSH, browser panes, reliability gates, and local arm64 packaging exist. Public distribution still needs Developer ID signing, notarization, stapling, and real macOS 13 hardware qualification. |
| Linux | Experimental, GPL-3.0-only. Phase 1 and GTK Slices 2.1, 2.2A, and 2.2B are complete: the engine passes headless and clean-runtime gates, one real terminal renders and closes correctly in GTK 4 over X11 with Mesa llvmpipe, and keyboard input reaches it with fixed byte expectations including Compose, dead keys, AltGr, a non-US layout, and a real IME preedit/commit flow. Wayland, scaling, fairness, WebKit coexistence, selection/clipboard/mouse/search, x86_64, and native packaging remain unproven. This repository cannot currently be built standalone — see ADR 0008 R1. |

Linux is not yet a production application or downloadable desktop package.
GTK is the leading toolkit candidate, not a final selection.

## Architecture

```text
platform host
  macOS: Swift + AppKit              Linux candidate: GTK 4
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

1. Finish the Phase 2 kill tests: native Wayland, the bounded WebKitGTK
   coexistence probe, scaling, and event fairness. Select GTK only if they
   pass.
2. Freeze shared portable fixtures, then build the pure Linux product model.
   This is the critical path to a shippable alpha and it is currently the
   longest-open item.
3. Build the terminal-first desktop alpha, including the selection, clipboard,
   paste-safety, mouse, and search behavior that moved out of Phase 2.
4. Add multiplexer behavior, reliability, and native packages.
5. Consider browser panes only after the terminal product is stable.

The exact resume order is in [NEXT_STEPS.md](NEXT_STEPS.md). Proven evidence
and current blockers are in [PORT_STATUS.md](PORT_STATUS.md).

## Repository direction

The recommended destination is one private monorepo with separate platform
trees and root-level contracts/fixtures. The landing page should remain a
separate deployment repository. No repository migration has been performed.
See the [migration proposal](docs/MONOREPO_MIGRATION.md).

## Licensing

**The Linux host is GPL-3.0-only.** See [`LICENSE`](LICENSE) and
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
