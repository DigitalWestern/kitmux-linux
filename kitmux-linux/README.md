# Kitmux Linux proof workspace

This directory holds the Linux engine, GTK proof harness, Rust product model,
and one-terminal product application. The Linux port is experimental; GTK 4
is the selected host toolkit.

Current scope:

1. Materialize the locked libkitty reference.
2. Build `libkitty.so` against Linux CPython.
3. Compile the public header from C and C++.
4. Port headless lifecycle and PTY tests.
5. Run the bounded GTK 4/libkitty toolkit spike.
6. Build the display-free Rust product model and bounded host contracts
   against frozen portable fixtures.
7. Prove cross-host fixture compatibility and preview macOS state without
   writing it or executing saved commands.
8. Build and gate the release-shaped Rust/GTK terminal multiplexer alpha.
9. Provide secure local control and the release-runtime `kitmuxctl` CLI.

No browser UI or native package installer belongs here yet. SSH and resume
workflows remain later Phase 6 slices. For
complete VM lifecycle, gates, release/SBOM commands, loader architecture, and
limitations, see
[`../docs/LINUX_DEVELOPMENT.md`](../docs/LINUX_DEVELOPMENT.md).

## Display-free model

`rust/model` owns stable workspace/group/tab/pane/surface/split identities,
split geometry, navigation and reorder rules, close cascading, abstract
terminal/browser runtime ownership, bounded state/settings/control codecs,
command semantics, SSH review data, a read-only macOS-state import preview,
and small Linux filesystem/path adapters. It has no display, libkitty, WebKit,
shell-execution, or network-runtime dependency. Run the Rust-only gate from
the repository root:

```sh
kitmux-linux/scripts/test-model.sh
```

Run the complete Phase 3 gate, including macOS consumption of Linux-produced
portable values:

```sh
kitmux-linux/scripts/test-phase3.sh
```

Preview a copied macOS `state.json` against an existing Linux home directory:

```sh
cargo run --locked \
  --manifest-path kitmux-linux/rust/model/Cargo.toml \
  --bin kitmux-import-preview -- \
  /path/to/macos-state.json /home/example
```

The command prints accepted, translated, rejected, and inert-command fields
as JSON. It never writes the source or executes a command; it is not a live
import or restore tool.

## Current Ubuntu workflow

On the macOS host, materialize and verify the locked reference:

```sh
kitmux-linux/scripts/materialize-reference.sh
```

Inside the Ubuntu VM:

```sh
kitmux-linux/scripts/provision-ubuntu.sh
kitmux-linux/scripts/test-headless.sh
```

The test command builds the pinned Kitty native extension, builds
`libkitty.so`, audits its ELF runpath and exports, runs the C/C++ header,
engine-lifecycle, full session API, Linux flood/close/reaping/resource suites,
then checks the public struct layout from Rust.

The pinned headless VM definition is `headless/lima.yaml`. Start and stop it
from the repository root with `limactl start kitmux-linux` and
`limactl stop kitmux-linux`.

## Linux desktop VM

The desktop environment is a separate Lima VM so GUI dependencies and display
services cannot destabilize the fast headless gate. From this repository root
on the macOS host:

```sh
limactl start --yes \
  --name=kitmux-linux-desktop \
  --mount-only="$PWD:w" \
  kitmux-linux/desktop/lima.yaml
limactl shell kitmux-linux-desktop -- \
  "$PWD/kitmux-linux/scripts/provision-desktop-ubuntu.sh"
limactl shell kitmux-linux-desktop -- \
  "$PWD/kitmux-linux/scripts/start-desktop.sh"
```

Open [http://127.0.0.1:6080/vnc.html](http://127.0.0.1:6080/vnc.html) on the
host. The no-password VNC endpoint is bound to guest localhost and forwarded
only to host localhost; it is for local development, not remote access.

Run the repeatable X11, noVNC, GTK 4, and OpenGL environment gate with:

```sh
limactl shell kitmux-linux-desktop -- \
  "$PWD/kitmux-linux/scripts/test-desktop.sh"
```

The desktop gate builds and launches real libkitty shells in `GtkGLArea` and
checks rendering, resize, tracked GL state, child reaping, keyboard/IME,
native Wayland, and bounded WebKitGTK coexistence behavior. Its scaling run
moves one live session across nested Sway outputs at 100%, 150%, and 200%,
asserts exact surface/backing scales, framebuffer and cell metrics, and
returns to the original 100% state without session loss. Proof images are
listed in [`../docs/LINUX_DEVELOPMENT.md`](../docs/LINUX_DEVELOPMENT.md).

The current VM uses Mesa `llvmpipe`, nested Weston, and nested Sway/pixman.
It proves the tested X11 and native-Wayland client paths, including virtual
fractional scaling. It does not prove physical-GPU performance, physical
mixed-DPI hardware, or packaging.

Run the release-shaped one-terminal product gate against an existing display:

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase4.sh"
```

The Phase 4 gates prove the passwd shell,
PTY/render/resize/title/cwd lifecycle, isolated relative loader paths, ordered
exit, child reaping, and terminal interaction on X11 and native Wayland. Both
backends carry external clipboard evidence; X11 carries foreground-close
confirmation evidence.
The Slice 4.3 persistence gate uses isolated XDG roots to prove private atomic
state, safe cwd/font restore into a fresh shell, inert saved commands,
replacement watching, recovery policy, and real full-disk preservation.

The Linux settings file can override the seven currently implemented terminal
shortcuts by stable command ID without changing the portable settings schema:

```json
{
  "linuxShortcutBindings": {
    "terminal.find": {"key": "g", "control": true, "shift": true}
  }
}
```

Supported IDs are `terminal.copy`, `terminal.paste`, `terminal.find`,
`terminal.clear-scrollback`, `font.increase`, `font.decrease`, and
`font.reset`. Omitted or invalid entries keep their defaults; ambiguous chords
are not consumed, and Control-only overrides are rejected so terminal control
sequences remain reachable. Modifier fields are `control`, `shift`, `alt`, and
`super`.

Run the focused product gates from the host:

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase4-wayland.sh"
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase4-persistence.sh"
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase4-programs.sh"
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase4-soak.sh"
kitmux-linux/scripts/test-phase4-clean-target.sh
```

These add native-Wayland interaction, crash/failure persistence, explicit
Bash/Zsh/Fish/Vim/Less/tmux input, the 30-minute PTY/resize/interaction soak,
and a release launch in a runtime-only Ubuntu container with no SDK. The last
gate is clean-runtime evidence, not native package or desktop-install proof.

## Secure local control and CLI

The release-shaped app exposes a private Unix socket and the matching
`kitmuxctl` executable. The socket uses a private XDG runtime path, mode `0600`,
owner/type/symlink checks, Linux peer credentials, bounded newline-delimited
frames, slow-client timeouts, and bounded event history.

Run the X11 control gate in the desktop VM:

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase6-control.sh"
```

The release runtime installs `bin/kitmuxctl`. For a user-local development
fallback, link a known CLI binary into the user's bin directory:

```sh
kitmux-linux/scripts/install-user-cli.sh /path/to/runtime/bin/kitmuxctl
```

The script prints the destination and warns when the destination directory is
not on `PATH`; it does not alter shell configuration.

## Release-shaped engine runtime

Inside the headless Ubuntu VM:

```sh
runtime="kitmux-linux/build/kitmux-engine-runtime-$(date +%Y%m%d-%H%M%S)"
kitmux-linux/scripts/build-release-runtime.sh "$runtime"
kitmux-linux/scripts/test-release-runtime.sh "$runtime"
```

This creates a release proof tree at the supplied path, packages the pinned
Python and Kitty runtime, copies only the transitive native-library closure,
applies relative ELF runpaths, rejects developer checkout paths, and runs the
Linux stress suite both before and after moving the tree. Fresh clean-gate
outputs were approximately 104 MiB. Ignored local outputs may be older; use a
new destination and do not infer release state from an existing build
directory. The runtime does not use `LD_LIBRARY_PATH`.

The tree includes exact per-component notices, the source/component manifest,
the resolved system/bundled SONAME report, an SPDX 2.3 JSON SBOM with per-file
checksums, and a complete `SHA256SUMS`. The audit fails on missing attribution,
unowned payload files, stale SBOM hashes, undeclared native dependencies, or a
host Python resolution.

The slower clean-distribution gate builds an isolated copy of the current
Git-visible worktree twice each in fresh Ubuntu 26.04 and Fedora 44 userspaces:

```sh
kitmux-linux/scripts/test-clean-containers.sh
```

Each distribution's two complete release inventories must be byte-identical.
The headless development Kitty bundle uses `LD_LIBRARY_PATH` for its downloaded
native dependencies. That shortcut is confined to headless engine tests and
must never enter the GTK process. The GTK host isolates only `libpython` and
uses distro GTK/text/graphics libraries. A release runtime must use an
isolated, relocatable `$ORIGIN` layout.
