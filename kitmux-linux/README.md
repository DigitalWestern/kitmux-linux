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
engine-lifecycle, and full session API suites, then checks the public struct
layout from Rust.

The development Kitty bundle uses `LD_LIBRARY_PATH` for its downloaded native
dependencies. That shortcut is intentionally confined to tests. A release
runtime must use an isolated, relocatable `$ORIGIN` layout.
