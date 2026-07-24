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
| Linux | Experimental. Phase 1 and GTK Slice 2.1 are complete: the engine passes headless and clean-runtime gates, and one real terminal renders and closes correctly in GTK 4 over X11 with Mesa llvmpipe. Input, Wayland, scaling, fairness, WebKit coexistence, x86_64, and native packaging remain unproven. |

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

1. Complete Linux Slice 2.2 input and interaction proof.
2. Complete Slice 2.3 on Wayland/X11, scaling, real GPU, fairness, and the
   bounded WebKitGTK coexistence probe; select GTK only if those gates pass.
3. Freeze shared portable fixtures, then build the pure Linux product model.
4. Build the terminal-first desktop alpha.
5. Add multiplexer behavior, reliability, and native packages.
6. Consider browser panes only after the terminal product is stable.

The exact resume order is in [NEXT_STEPS.md](NEXT_STEPS.md). Proven evidence
and current blockers are in [PORT_STATUS.md](PORT_STATUS.md).

## Repository direction

The recommended destination is one private monorepo with separate platform
trees and root-level contracts/fixtures. The landing page should remain a
separate deployment repository. No repository migration has been performed.
See the [migration proposal](docs/MONOREPO_MIGRATION.md).

## Licensing

Kitty and `libkitty` are GPL-3.0-only. The verified Linux engine runtime
includes per-component notices and a machine-readable SPDX 2.3 SBOM. The
runtime's component manifest is
[`kitmux-linux/release/runtime-components.json`](kitmux-linux/release/runtime-components.json).

The combined product does not yet have a root license declaration in this
repository. Before public source or binary distribution, the owner should
confirm the intended project-wide license, source-offer obligations, notices,
and package-specific compliance. The Linux SBOM proves the current engine
payload inventory; it is not legal approval for a future desktop package.
