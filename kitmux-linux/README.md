# Kitmux Linux proof workspace

This directory holds proof code until the engine and GTK gates justify a
production application.

Current scope:

1. Materialize the locked libkitty reference.
2. Build `libkitty.so` against Linux CPython.
3. Compile the public header from C and C++.
4. Port headless lifecycle and PTY tests.
5. Run the disposable GTK 4 rendering spike.

No browser, package installer, or production navigation shell belongs here
yet.

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

The current VM uses Mesa `llvmpipe`. It proves functional desktop rendering,
not physical-GPU performance or final GNOME/Wayland compatibility.

## Release-shaped engine runtime

Inside the headless Ubuntu VM:

```sh
kitmux-linux/scripts/build-release-runtime.sh
```

This creates a roughly 104 MB proof tree under
`kitmux-linux/build/kitmux-engine-runtime`, packages the pinned Python and
Kitty runtime, copies only the transitive native-library closure, applies
relative ELF runpaths, rejects developer checkout paths, and runs the Linux
stress suite both before and after moving the tree. It does not use
`LD_LIBRARY_PATH`.

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
The development Kitty bundle uses `LD_LIBRARY_PATH` for its downloaded native
dependencies. That shortcut is intentionally confined to tests. A release
runtime must use an isolated, relocatable `$ORIGIN` layout.
