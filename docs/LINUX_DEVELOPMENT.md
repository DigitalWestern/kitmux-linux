# Linux development and evidence

This is the operational reference for the experimental Linux port: how to build
it, how to run every gate, and where its architectural boundaries are. Run all
host commands from the Linux repository root on the macOS machine.

**What is currently proven, and what is not, lives in
[`../PORT_STATUS.md`](../PORT_STATUS.md)** — its evidence log carries the exact
per-slice commands and results, and its "Current blockers and limits" section
carries the non-claims. This file does not restate them; when the two disagree,
`PORT_STATUS.md` wins.

The short version: `libkitty.so`, the headless engine suite, the release-shaped
engine runtime, the display-free Rust model, and the GTK terminal multiplexer
alpha all pass on Ubuntu 26.04 ARM64 with Mesa llvmpipe over X11 and a native
Wayland client path. Nothing here proves a physical GPU, physical input, x86_64,
mixed-DPI hardware, complete AT-SPI coverage, browser product behavior, or a
native package.

## Repository layout

- `kitmux-linux/rust/model` — display-free product model. Stable
  workspace/group/tab/pane/surface/split identities, split geometry, navigation
  and reorder rules, close cascading, abstract terminal/browser runtime
  ownership, bounded state/settings/control codecs, command semantics, SSH
  review data, a read-only macOS-state import preview, and small Linux
  filesystem/path adapters. No display, libkitty, WebKit, shell-execution, or
  network-runtime dependency.
- `kitmux-linux/rust/app` — the release-shaped Rust/GTK product application and
  the `kitmuxctl` control client.
- `kitmux-linux/src` — durable C: `gtk_key_translation.{c,h}` and
  `gtk_terminal_bridge.{c,h}` behind the FFI boundary, plus the disposable
  `gtk_terminal_host.c` spike (ADR 0007).
- `kitmux-linux/tests` — C harnesses: key matrix, PTY input recorder, X11 key
  injector, header compile checks, session stress.
- `kitmux-linux/scripts` — every gate and build entry point named below.
- `kitmux-linux/headless/lima.yaml`, `kitmux-linux/desktop/lima.yaml` — the two
  pinned VM definitions.

No browser UI or native package installer belongs here yet.

## Report reference drift

Run this at every phase boundary:

```sh
kitmux-linux/scripts/report-reference-drift.py
```

The Markdown report compares the tag and commit in `source-lock.json` with
current macOS `HEAD`, restricted to the reusable paths named in
`LINUX_PORT_PLAN.md`. It classifies committed changes as contract-affecting,
behavior-affecting, or irrelevant to Linux, and reports relevant uncommitted
files separately. Use `--patch` to append the full restricted diff.

A change to `libkitty/include/libkitty.h` or `patches/` requires a new
baseline. Other contract or behavior drift requires review; a macOS view-only
change does not require rebaselining. Record the decision in `PORT_STATUS.md`,
including "no relevant drift".

When drift is mandatory or deliberately accepted, first test and tag a clean
macOS `HEAD`. Then start the headless VM and run this from a clean Linux tree:

```sh
kitmux-linux/scripts/report-reference-drift.py --relock NEW_TAG
```

The guarded command requires `NEW_TAG` to name macOS `HEAD`, updates only
`source-lock.json`, materializes the lock, and runs
`kitmux-linux/scripts/test-headless.sh` in the Ubuntu VM. Review and commit
only `source-lock.json`; record the new baseline and result separately in
`PORT_STATUS.md`.

## Materialize the locked source

The Linux tree carries a durable mirror of the locked libkitty reference under
`kitmux-linux/locked-inputs/reference`. Materialization verifies that mirror
and applies the hash-locked Linux render-scale overlay in
`kitmux-linux/patches/libkitty/`. A checkout from before the mirror was added
may still fall back to the tagged adjacent macOS checkout; a standalone build
must use the durable mirror.

Run this once after cloning or after deleting the ignored `.source` cache:

```sh
kitmux-linux/scripts/materialize-reference.sh
```

The locked Kitty dependency bundles are too large for git history, so they
live as assets on the `dependency-bundles-v1` GitHub release. The script below
downloads any missing bundle into the ignored local mirror at
`kitmux-linux/locked-inputs/dependency-bundles`, then copies it into the
development cache:

```sh
kitmux-linux/scripts/materialize-dependencies.sh
```

Both scripts verify SHA-256 values from `source-lock.json` before materializing
anything. The exact x86_64 bundle remains unavailable while the upstream
rolling URL serves a different digest; do not replace the lock with that file.

## Headless VM

Create and provision the pinned Ubuntu ARM64 VM:

```sh
limactl start --yes \
  --name=kitmux-linux \
  --mount-only="$PWD:w" \
  kitmux-linux/headless/lima.yaml
limactl shell kitmux-linux -- \
  "$PWD/kitmux-linux/scripts/provision-ubuntu.sh"
```

Normal lifecycle:

```sh
limactl start kitmux-linux
limactl shell kitmux-linux
limactl stop kitmux-linux
```

Stopping preserves the disk. `limactl delete kitmux-linux` destroys the VM and
is intentionally not part of the normal workflow.

Run the fast headless gate from the host:

```sh
limactl shell kitmux-linux -- \
  "$PWD/kitmux-linux/scripts/test-headless.sh"
```

It builds the pinned Kitty native extension, builds `libkitty.so`, audits its
ELF runpath and exports, runs the C/C++ header, engine-lifecycle, full session
API, and Linux flood/close/reaping/resource suites, then checks the public
struct layout from Rust.

Run the standalone R1/R2 gate inside Linux or CI:

```sh
kitmux-linux/scripts/test-standalone.sh
```

It deliberately points `KITMUX_MACOS_REPO` at a missing path, verifies the
durable mirrors, rebuilds the pinned headless engine, and runs the source,
header, fixture, and model checks. The GitHub Actions trigger is
`.github/workflows/linux-standalone.yml`; a local run is evidence for the
checkout only until that workflow has run on its configured runner.

## Package and installation gates

Build a release-shaped runtime first, then promote it in this order:

```sh
KITMUX_BUILD_APP_RUNTIME=1 KITMUX_APP_TEST_HOOKS=OFF \
  kitmux-linux/scripts/build-release-runtime.sh /tmp/kitmux-runtime
SOURCE_DATE_EPOCH=0 kitmux-linux/scripts/package-tarball.sh \
  /tmp/kitmux-0.1.0-arm64.tar.xz /tmp/kitmux-runtime
SOURCE_DATE_EPOCH=0 kitmux-linux/scripts/package-deb.sh \
  /tmp/kitmux_0.1.0_arm64.deb /tmp/kitmux-runtime
```

Use a fresh Linux VM for the lifecycle gate. It requires an X11 `DISPLAY` and
noninteractive `sudo` for `dpkg`:

```sh
limactl shell kitmux-linux-package-20260818 -- env DISPLAY=:1 \
  bash "$PWD/kitmux-linux/scripts/test-package-lifecycle.sh" \
  /tmp/kitmux-0.1.0-arm64.tar.xz /tmp/kitmux_0.1.0_arm64.deb
```

The gate verifies tarball launch, `.deb` install and launch, upgrade,
downgrade, reinstall, and uninstall. It does not prove x86_64, signing,
vulnerability scanning, a desktop-menu click, or physical-GPU behavior.

## Complete gate sequence

Run the dependency-ordered host, headless-VM, and desktop-VM gates with one
command:

```sh
kitmux-linux/scripts/test-all.sh
```

The command validates the feature inventory once at the aggregate boundary;
nested gates inherit that result, while a standalone gate still validates when
run directly. It uses `/tmp` for release build state by default and stops at
the first failure. `--list` prints the exact sequence without running a gate or
validation.
Set `KITMUX_DISPLAY` to choose the desktop VM display and
`KITMUX_PHASE4_SOAK_SECONDS` to shorten or extend the soak deliberately.

## Display-free Phase 3 gate

Run the Rust model plus the cross-host macOS fixture consumer from the macOS
host:

```sh
kitmux-linux/scripts/test-phase3.sh
```

Run the same Rust source gate offline in the Ubuntu VM:

```sh
limactl shell kitmux-linux -- env CARGO_NET_OFFLINE=true \
  "$PWD/kitmux-linux/scripts/test-model.sh"
```

The cross-host gate generates compatibility values only in a temporary
directory. It does not change the canonical fixture corpus or its macOS mirror.

Preview a copied macOS `state.json` against an existing Linux home directory:

```sh
cargo run --locked \
  --manifest-path kitmux-linux/rust/model/Cargo.toml \
  --bin kitmux-import-preview -- \
  /path/to/macos-state.json /home/example
```

It prints accepted, translated, rejected, and inert-command fields as JSON. It
never writes the source or executes a command; it is not a live import or
restore tool.

## Desktop VM and GTK gate

Create and provision the separate pinned desktop VM:

```sh
limactl start --yes \
  --name=kitmux-linux-desktop \
  --mount-only="$PWD:w" \
  kitmux-linux/desktop/lima.yaml
limactl shell kitmux-linux-desktop -- \
  "$PWD/kitmux-linux/scripts/provision-desktop-ubuntu.sh"
```

Start XFCE, TigerVNC, and noVNC, then open
<http://127.0.0.1:6080/vnc.html>:

```sh
limactl start kitmux-linux-desktop
limactl shell kitmux-linux-desktop -- \
  "$PWD/kitmux-linux/scripts/start-desktop.sh"
```

Run the GTK gate:

```sh
limactl shell kitmux-linux-desktop -- \
  "$PWD/kitmux-linux/scripts/test-desktop.sh"
```

The gate may instead use an existing display and no noVNC endpoint:

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 KITMUX_NOVNC_PORT= \
  "$PWD/kitmux-linux/scripts/test-desktop.sh"
```

The gate covers rendering, resize, PTY, clean close, the Slice 2.2A/2.2B
keyboard and input-method harness over X11, the Slice 2.2C native Wayland
client path under nested Weston, and the Slice 2.2D static WebKitGTK
coexistence probe under both backends. Slice 2.2E then starts nested Sway with
100%, 150%, and 200% virtual outputs and moves one live session through all
three and back. It writes
`kitmux-linux/gtk-terminal-host-proof.png`,
`kitmux-linux/gtk-keyboard-focus-proof.png`, and
`kitmux-linux/gtk-preedit-proof.png`, plus
`kitmux-linux/gtk-wayland-proof.png`, plus
`kitmux-linux/gtk-webkit-x11-proof.png` and
`kitmux-linux/gtk-webkit-wayland-proof.png`, plus
`kitmux-linux/gtk-scale-100-proof.png`,
`kitmux-linux/gtk-scale-150-proof.png`, and
`kitmux-linux/gtk-scale-200-proof.png`. While it runs it changes X
auto-repeat, the keyboard layout, and the active IBus engine. Cleanup restores
the original repeat state/rate, complete XKB configuration, and active IBus
engine. A dedicated test display remains recommended because the changes are
visible while the gate runs.

Run the current release-shaped product gate against an existing display:

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase4.sh"
```

This builds a fresh app runtime, audits its relative loader closure, verifies
the child executable matches the passwd shell, and drives lifecycle plus the
Slice 4.2 interaction paths. It includes external clipboard round trips, unsafe
paste cancel/confirm, selection, search, font controls, mouse/wheel input,
pointer-time PTY progress, and child reaping. The X11 run also proves
foreground-close cancel/confirm.

Run the same product controller as a native Wayland client under nested Weston:

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase4-wayland.sh"
```

That run asserts `GdkWaylandDisplay` and replays input, resize, external
`wl-copy`/`wl-paste`, unsafe paste, selection, search, font, mouse, wheel,
pointer-time PTY progress, and child-exit/reaping. XTEST targets
Weston's outer X11 window, so it cannot issue a Wayland toplevel close; the X11
gate remains authoritative for the product close dialog.

Run the Slice 4.3 persistence gate:

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase4-persistence.sh"
```

It builds one fresh runtime and uses only temporary XDG roots. It performs the
two-launch and immediate-close fresh-shell restore tests, replacement/removal/
recreation watching, malformed-edit recovery, same-content suppression,
no-clobber corrupt/newer set-asides, and read-only cases, then mounts and fills
a private 64 KiB tmpfs to exercise real `ENOSPC`. Non-interactive `sudo` is
required only for that temporary mount and unmount; cleanup removes the mount
and all test state.

Run the Phase 4 shell/editor matrix and required 30-minute interaction soak:

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase4-programs.sh"
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase4-soak.sh"
```

The program gate explicitly enters Bash, Zsh, and Fish, then drives Vim,
Less, and tmux through the real terminal. The soak sustains PTY output for at
least 1,800 monotonic seconds while repeatedly resizing and sending pointer,
wheel, font, and shell-heartbeat input; it fails on a five-second heartbeat
miss or any surviving shell/flood child.

Run the clean no-SDK release launch from the macOS host:

```sh
kitmux-linux/scripts/test-phase4-clean-target.sh
```

That wrapper builds a fresh release runtime in the desktop VM, then launches
it under Xvfb in a runtime-only Ubuntu 26.04 container in the headless VM. It
rejects compilers, Cargo, CMake, Make, pkg-config, and GTK/GLib/Epoxy development
packages. This is clean runtime evidence, not a desktop installation or native
package test.

## Phase 5 multiplexer gates

Both gates build their own fresh release runtime into a temporary directory and
require an existing X11 display:

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase5-product.sh"
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase5-navigation.sh"
```

`test-phase5-product.sh` uses isolated XDG config/state/data/cache roots. It
drives the in-window GTK menu bar with F10, arrows, and Escape, then drives the
command palette and settings dialog by keyboard only, builds a
two-workspace nested five-session hierarchy through the product shortcut and
palette paths, checks that a foreground job in a non-active group is still
reviewed when that group closes, and corrupts the primary state file to prove
the last readable hierarchy is recovered before any new write.

`test-phase5-navigation.sh` covers nested splits, divider drag, directional and
cycle focus, keyboard resize, and hidden-tab output ownership. Two optional
environment toggles select extra runs: `KITMUX_RAPID_NAV_GATE=1` for the rapid
navigation pass and `KITMUX_ACCESSIBILITY_GATE=1` for the roles/labels and
focus-order pass. Set `GDK_BACKEND` and `WAYLAND_DISPLAY` to run the same gate
as a native Wayland client under nested Weston.

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

If the runtime socket file is removed while Kitmux is still running, restart
Kitmux or set `KITMUX_SOCKET_PATH` to a stable private path. The server does not
auto-rebind it.

## SSH and agent workflows

The Linux alpha stores bounded SSH profiles at
`$XDG_CONFIG_HOME/kitmux/ssh-profiles.json` (or the path supplied by the
absolute `KITMUX_SSH_PROFILES_PATH` override). It resolves the user's
PATH-selected `ssh` executable with `ssh -G`, shows a fingerprinted review for
externally listening forwards, and launches only the reviewed argument array.
The profile store is atomic and private (`0600`), and corrupt input is moved to
a timestamped quarantine file. `SSH_AUTH_SOCK` is passed through when present;
its absence is reported without logging the socket path.

Run the ARM64 X11 acceptance gate against a fresh release runtime:

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase6-ssh.sh"
```

The gate covers `kitmuxctl ssh profile list`, review then approval for
`ssh.connect`, explicit reconnect review/approval for a disconnected SSH pane,
nonstandard executable lookup, one-argument remote commands, missing agent
environment, private profile permissions, and log-safety checks.

## Resume and recovery

Saved terminal commands are inert metadata. On restore they are shown in an
unchecked review dialog; saved SSH profiles restore as disconnected placeholders
and never start a session. The Run action revalidates the pane, command, cwd,
and eligibility immediately before writing to a live non-SSH terminal.

Run the ARM64 X11 persistence, crash, stale-socket, SSH, upgrade-compatibility,
and stress gate with a fresh release runtime:

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase6-resume.sh"
```

The `KITMUX_AUTORESUME` modes used by this gate exist only in test-hook builds;
they are not an approval mechanism in a release build. Normal startup never
auto-runs restored commands or SSH sessions.

## Terminal shortcut overrides

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

## Keyboard harness structure

The keyboard harness has two halves:

- `gtk_key_matrix` needs no display. It drives the GDK-to-libkitty
  translation and kitty's encoder over a fixed key table in three live
  terminal states (default, DECCKM, and kitty keyboard protocol flags 15),
  then checks the bytes a real PTY child (`pty_input_recorder`) actually
  read.
- Four windowed runs replay fixed key scripts through real GDK events using
  `x11_key_injector`, a small XTEST tool: the default terminal state, the
  kitty keyboard protocol, GTK's own input-method context (Compose, dead
  keys, AltGr, a German layout), and IBus (`m17n:t:latn-post` preedit and
  commit, plus a pass-through engine). `xdotool` cannot be used for the
  function keys: it infers modifiers from the shift level where it finds a
  keysym, and this session's XKB map puts `XF86Switch_VT_*` on higher levels
  of the same keycode, so `xdotool key F1` arrives as Alt+F1 and XFCE
  consumes it.
- The Wayland run starts Weston without Xwayland and asserts that the GTK host
  reports `GdkWaylandDisplay`. Weston is nested in the visible X11 desktop.
  XTEST drives only Weston's outer window; Weston delivers native
  `wl_keyboard` events to GTK. This is intentionally display-bound
  compositor-bridge evidence, not a physical libinput/evdev claim.
- The two WebKit runs load only a static in-memory page, move focus into the
  web view and back, assert exact terminal bytes, and reject missing loader
  objects, WebKit process termination, or leakage of Kitty's private native
  dependency directory. They prove coexistence, not browser functionality.
- The scaling run checks both `gdk_surface_get_scale()` and GTK's integer
  backing factor. Kitty uses the backing factor `1/2/2` for its font atlas,
  while the compositor maps the logical surface at `1/1.5/2`. It asserts the
  same child survives `100% -> 150% -> 200% -> 100%` and restores the original
  framebuffer, cell, and grid metrics.

Stop desktop services and the VM without deleting either:

```sh
limactl shell kitmux-linux-desktop -- \
  "$PWD/kitmux-linux/scripts/stop-desktop.sh"
limactl stop kitmux-linux-desktop
```

The no-password VNC endpoint is bound to guest localhost and forwarded only to
host localhost. It is a local development service, not a remote-access setup.

## Release runtime, notices, and SBOM

Inside the headless VM, choose a new output path for every build. The builder
refuses to overwrite an existing tree so an ignored, stale build cannot be
mistaken for new evidence:

```sh
runtime="${TMPDIR:-/tmp}/kitmux-engine-runtime-$(date +%Y%m%d-%H%M%S)"
kitmux-linux/scripts/build-release-runtime.sh "$runtime"
kitmux-linux/scripts/test-release-runtime.sh "$runtime"
```

The generated artifacts include:

- `share/kitmux-engine.spdx.json` — SPDX 2.3 JSON SBOM;
- `share/runtime-components.json` — component/version/license/source manifest;
- `share/licenses/` and `share/THIRD_PARTY.md` — attribution;
- `share/RUNTIME_DEPENDENCIES.json` — bundled/system SONAME resolution;
- `share/SHA256SUMS` — complete regular-file inventory.

The audit fails on missing attribution, unowned payload files, stale SBOM
hashes, undeclared native dependencies, or a host Python resolution.

Old ignored runtime/build trees may come from an older builder and are not
evidence. Builders now default to `/tmp`; use a fresh output path and the
successful audit output.

Regenerate the tracked upstream notices only when changing the locked
component manifest:

```sh
python3 kitmux-linux/scripts/refresh-license-notices.py \
  --manifest kitmux-linux/release/runtime-components.json \
  --output kitmux-linux/release/licenses \
  --kitty-root .source/kitty
```

That command downloads exact source archives and checks their recorded
SHA-256 values. Review the resulting notice diff and then run a fresh runtime
build. The runtime builder generates the SBOM; it should not be hand-edited.

Run the slow clean/reproducibility gate inside the headless VM:

```sh
kitmux-linux/scripts/test-clean-containers.sh
```

It builds the current Git-visible candidate twice in Ubuntu 26.04 and twice in
Fedora 44 containers. Each distribution's two complete release inventories must
be byte-identical. It does not prove a desktop installation.

## Kitty/GTK loader boundary

Kitty's downloaded development bundle includes private builds of Cairo,
HarfBuzz, GLib, xkbcommon, and related native libraries. Adding that entire
directory to a GTK process with `LD_LIBRARY_PATH` or a broad ELF runpath can
silently replace the ABI-compatible distribution libraries that GTK loaded.
The first real GTK link exposed exactly this collision.

The development GTK host copies only the pinned
`libpython3.14.so.1.0` into `build-gtk/python-runtime` and links it through a
narrow runpath. GTK and the terminal renderer use the distribution's native
text, graphics, input, and desktop libraries. The release engine tree remains
self-contained and `$ORIGIN`-relative because it is tested separately.

The headless development Kitty bundle does use `LD_LIBRARY_PATH` for its
downloaded native dependencies. That shortcut is confined to headless engine
tests and must never enter the GTK process. A release runtime uses an isolated,
relocatable `$ORIGIN` layout and no `LD_LIBRARY_PATH` at all.

Any desktop package must preserve this boundary. If a self-contained package
cannot keep one-process library resolution safe, isolate the engine in a
separate process. Do not repair the problem with a global `LD_LIBRARY_PATH`.

## Known limitations and decision boundary

The local desktop VM is ARM64, XFCE/X11, and software-rendered with llvmpipe.
Nested Weston now proves the GTK client's native Wayland GL/input/IME path,
and nested Sway proves fractional scaling over three virtual unlike outputs.
Neither proves a physical Wayland desktop, libinput device, latency, frame
pacing, physical mixed-DPI monitors, or vendor GPU drivers.

PTY/frame fairness passed with five saturated sessions and bounded UI latency.
The minimal adjacent WebKitGTK probe also passed both display backends. WebKit
is not part of the terminal-first alpha; the probe exists only to expose
loader, GL, focus, and dependency conflicts early. The complete Phase 2 gate
selected GTK 4, which now owns the production Linux UI.
