# Kitmux Linux proof workspace

This directory holds the Linux engine, the durable C key/render bridge, the
disposable GTK proof harness, the display-free Rust product model, and the
release-shaped terminal multiplexer application. The Linux port is
experimental; GTK 4 is the selected host toolkit.

Everything operational — VM lifecycle, every gate command, the release runtime
and SBOM, the loader boundary, shortcut overrides, and the control socket — is
in [`../docs/LINUX_DEVELOPMENT.md`](../docs/LINUX_DEVELOPMENT.md). What is
proven and what is not is in [`../PORT_STATUS.md`](../PORT_STATUS.md).

Directory map:

| Path | Contents |
| --- | --- |
| `rust/model` | Display-free product model. No display, libkitty, WebKit, shell-execution, or network dependency. |
| `rust/app` | Release-shaped Rust/GTK application and the `kitmuxctl` client. |
| `rust/header-smoke` | Rust-side check of the public `libkitty` struct layout. |
| `src` | Durable C behind the FFI boundary, plus the disposable `gtk_terminal_host.c` spike (ADR 0007). |
| `tests` | C harnesses: key matrix, PTY input recorder, X11 key injector, header compiles, session stress. |
| `scripts` | Every build and gate entry point. |
| `headless`, `desktop` | The two pinned Lima VM definitions. |
| `patches` | The hash-locked Linux render-scale overlay applied during materialization. |
| `release` | Tracked component manifest and upstream notices. |
| `build*` | Ignored local output. Never release evidence — always build to a fresh path. |

No browser UI and no native package installer belong here yet. SSH and resume
workflows are later Phase 6 slices.
