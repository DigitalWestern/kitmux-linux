# Linux development and evidence

This is the operational reference for the experimental Linux port. Run all
host commands from the Linux repository root on the macOS machine.

## What is proven

- A pinned `libkitty.so` builds on Ubuntu 26.04 ARM64 and exports only its
  intended C API.
- Six headless C/C++/ELF/engine/session/stress tests and the Rust/C layout
  check pass.
- A relocatable release-shaped engine runtime passed twice in clean Ubuntu
  26.04 and twice in clean Fedora 44 ARM64 containers. Each distribution
  produced a stable repeated inventory.
- Fresh clean-gate outputs were approximately 104 MiB, with 24 SPDX packages,
  876 regular files, and 23 upstream notices. The SPDX 2.3 JSON document
  passed the official `spdx-tools` validator.
- One real libkitty terminal renders, pumps PTY output, resizes, restores
  tracked GL state, reports initialization failure, closes, and reaps its
  child in GTK 4.22.4 over X11 and a native Wayland client path with Mesa
  llvmpipe.
- Keyboard input reaches that terminal with exact, fixed byte expectations:
  press, release, and auto-repeat; the kitty keyboard protocol and DECCKM;
  Compose, dead keys, AltGr, a non-US layout, emoji, and a real IBus
  preedit/commit flow; and focus transfer to an ordinary GTK control beside
  the terminal.
- WebKitGTK 2.52.3 renders a static in-memory fixture beside that terminal
  under X11 and native Wayland without GL-state, loader, or focus conflict.
  The live host resolved 139 native libraries while keeping pinned libpython
  isolated from the distro WebKitGTK/GTK dependency set.
- Under nested Sway, one native-Wayland session moves through virtual outputs
  at 100%, 150%, and 200% and back to 100%. Exact surface/backing scales,
  framebuffer, cell, grid, content, and child-PID assertions pass without
  session or return-metric drift.

## What is not proven

- Selection, clipboard, safe paste, mouse reporting, wheel, touchpad, or
  search in the GTK host.
- CJK or other conversion engines with candidate windows, and
  surrounding-text requests; the input-method evidence covers one Latin
  engine.
- A physical Wayland desktop/libinput path, physical mixed-DPI monitors,
  simultaneous windows on unlike scales, physical-GPU behavior, or
  accessibility. The fractional result uses virtual nested outputs and
  software rendering.
- Main-loop fairness during sustained PTY output and resize pressure.
- Browser navigation, web data-session policy, web-process recovery, or a
  packaged WebKitGTK dependency layout. The bounded coexistence probe is not
  browser product evidence.
- x86_64 build, GUI, clean-runtime, or package gates.
- `.deb`, RPM, AppImage, desktop installation, launcher, upgrade/uninstall,
  sandbox, signing, or public distribution.

## Materialize the locked source

The Linux spike reads only hash-locked files from the tagged macOS baseline.
Materialization also verifies and applies the hash-locked Linux render-scale
overlay in `kitmux-linux/patches/libkitty/`; it does not create a separately
maintained libkitty copy. Run this once after cloning or after deleting the
ignored `.source` cache:

```sh
kitmux-linux/scripts/materialize-reference.sh
```

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
auto-repeat, the keyboard layout, and the active IBus engine — restoring
auto-repeat and the US layout on exit — so run it on the project VNC session
rather than a desktop you are using.

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
runtime="$PWD/kitmux-linux/build/kitmux-engine-runtime-$(date +%Y%m%d-%H%M%S)"
kitmux-linux/scripts/build-release-runtime.sh "$runtime"
kitmux-linux/scripts/test-release-runtime.sh "$runtime"
```

The generated artifacts include:

- `share/kitmux-engine.spdx.json` — SPDX 2.3 JSON SBOM;
- `share/runtime-components.json` — component/version/license/source manifest;
- `share/licenses/` and `share/THIRD_PARTY.md` — attribution;
- `share/RUNTIME_DEPENDENCIES.json` — bundled/system SONAME resolution;
- `share/SHA256SUMS` — complete regular-file inventory.

The current ignored `kitmux-linux/build/kitmux-engine-runtime` directory may
come from an older builder. Its size or contents are not evidence. Use a fresh
output path and the successful audit output.

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
Fedora 44 containers. It does not prove a desktop installation.

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

Any desktop package must preserve this boundary. If a self-contained package
cannot keep one-process library resolution safe, isolate the engine in a
separate process. Do not repair the problem with a global `LD_LIBRARY_PATH`.

## Known limitations and decision boundary

The local desktop VM is ARM64, XFCE/X11, and software-rendered with llvmpipe.
Nested Weston now proves the GTK client's native Wayland GL/input/IME path,
and nested Sway proves fractional scaling over three virtual unlike outputs.
Neither proves a physical Wayland desktop, libinput device, latency, frame
pacing, physical mixed-DPI monitors, or vendor GPU drivers.

The remaining Phase 2 check is PTY/frame fairness. The minimal adjacent
WebKitGTK probe passed both display backends. WebKit is not part of the
terminal-first alpha; the probe exists only to expose loader, GL, focus, and
dependency conflicts early.

GTK becomes the production choice only after the complete decision gate
passes on the support matrix. A concrete technical failure triggers one
equivalent, time-boxed Qt 6 comparison. Until then, do not build product UI
whose ownership depends on GTK.
