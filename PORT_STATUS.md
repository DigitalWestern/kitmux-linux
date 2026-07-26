# Kitmux Linux Port Status

**Last inspected:** 2026-07-25

**Implementation state:** Phase 0.4, Phase 1, and GTK Slices 2.1 through 2.2D
are closed. Separate Ubuntu ARM64 headless and XFCE desktop VMs, an ELF
`libkitty.so`, a relocatable 104 MB attributed engine runtime, Linux stress
tests, a real one-session GTK 4 terminal host over X11 and native Wayland
client paths, a deterministic keyboard and input-method harness, and an
authoritative portable contract corpus exist. No Linux product model,
navigation UI, or native package exists yet.

**Active phase:** Phase 2 — rendering and toolkit kill spike.

**Next slice:** Slice 2.2E — **scaling**. Phase 2 was
reordered on 2026-07-25 so the checks that could disqualify GTK run before the
expensive ones that almost certainly cannot. Selection, clipboard, safe paste,
mouse, wheel, and search moved to Phase 4 Slice 4.2 as product work. The
remaining Phase 2 slices are scaling and event fairness. Phase 0.4 shared
fixture promotion is closed, so Phase 3 is no longer
fixture-blocked; it remains unstarted while the assigned agent works the
current Phase 2 slice.
The concrete sub-slice order is in [`NEXT_STEPS.md`](NEXT_STEPS.md).
Operational VM, gate, runtime, and SBOM commands are in
[`docs/LINUX_DEVELOPMENT.md`](docs/LINUX_DEVELOPMENT.md).

**Licence:** GPL-3.0-only, decided 2026-07-25 in ADR 0006. `LICENSE` is at the
repository root.

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
- GTK 4 is the first toolkit candidate, not a final choice. The disposable
  host is C to test the direct libkitty/GTK boundary; Rust remains the future
  model/lifecycle candidate after shared fixtures are authoritative. Input,
  scale, display-backend, and event-loop evidence decides the production
  stack.
- The first user-facing target is a terminal-only alpha. Browser panes and
  broad distribution support are later decisions.

## Evidence log

### 2026-07-25 — Slice 2.2D WebKitGTK conflict probe

- Added WebKitGTK 6.0 only to the disposable GTK host and mapped one
  `WebKitWebView` beside the live `GtkGLArea`. It loaded static in-memory HTML;
  the probe has no network request, navigation UI, browser chrome, or product
  data-session policy.
- WebKitGTK 2.52.3 loaded the fixture under `GdkX11Display` and native
  `GdkWaylandDisplay` while the real libkitty recorder remained visible. Kitty
  restored the tracked host GL state after every draw, and WebKit reported no
  load failure or web-process termination.
- Focus moved from the terminal into WebKit and back under both backends. The
  child received `61`, received nothing while an injected `x` targeted the
  focused web view, then received `7a` after terminal focus returned. Including
  the close sentinel, both exact streams were `617a1b5b32347e`; both terminal
  children were reaped.
- The host directly needs `libwebkitgtk-6.0.so.4`. Its live ELF closure
  resolved 139 libraries with none missing: `libpython3.14.so.1.0` stayed in
  `build-gtk/python-runtime`, while WebKitGTK, JavaScriptCoreGTK, and GTK came
  from Ubuntu's architecture library directory. Kitty's private development
  dependency directory did not enter the process closure.
- Installing the Ubuntu WebKitGTK development input added 43 distro packages
  and 166 MB on this VM. The principal runtime objects observed were about
  93 MB for WebKitGTK and 30 MB for JavaScriptCoreGTK. This is dependency-cost
  evidence, not a packaged-application measurement.
- The desktop startup now publishes its VNC display to the systemd user
  session and starts the GTK desktop portal reproducibly. This avoids a
  20-second failed portal lookup per WebKit-linked process after a VM restart.
  The TigerVNC session check also accepts both `1` and `:1`, making repeated
  startup idempotent.
- GUI-tested: `kitmux-linux/scripts/test-desktop.sh` passed every existing X11,
  deterministic keyboard, Compose/layout, IBus, and native Wayland gate plus
  the two WebKit coexistence runs. Visible evidence is
  `kitmux-linux/gtk-webkit-x11-proof.png` and
  `kitmux-linux/gtk-webkit-wayland-proof.png`.
- Source-tested: `kitmux-linux/scripts/test-headless.sh` remained green: six
  tests passed in 6.75 seconds plus the Rust/C header layout check.
- Package-tested and clean-machine desktop-tested: none. The result is ARM64,
  Mesa llvmpipe, one VNC/XFCE VM, and one nested Weston compositor; it does not
  prove a physical GPU, another distro, x86_64, browser product behavior, or a
  distributable dependency layout.
- Slice 2.2D is closed. Slice 2.2E scaling is next; event fairness remains
  Slice 2.2F. GTK is still a candidate until the complete Slice 2.3 gate.

### 2026-07-25 — Slice 0.4 portable contract fixtures

- Added an authoritative `contracts/fixtures/v1/` corpus: one manifest and six
  versioned contract files with 20 named accept, repair, reject, and ignore
  cases. The files cover state snapshots/stable IDs, settings defaults and
  validation, split-tree collapse/close order, 38 stable command identifiers,
  control framing/limits, and portable SSH profile/review data.
- Every contract file declares bounds and malformed-input behavior. Accepted
  fixtures exclude user paths, installed fonts, display shortcut encodings,
  browser data, shell/agent commands, sockets, keychain references, and
  package paths. The one rejected SSH security case contains a newline-bearing
  command only to prove validation drops it.
- The canonical corpus lives in this Linux repository. The tagged macOS
  reference carries a temporary SwiftPM test-resource mirror. The validator
  byte-compares every declared JSON file and rejects an extra or missing file,
  so the mirror cannot drift silently before the monorepo removes it.
- Added eight `PortableContractFixtureTests`: consumers exercise the production
  snapshot, settings, split, command, control, and SSH validators; producers
  build native macOS values and compare them semantically to the fixtures.
  UUID hex case is normalized because Foundation emits uppercase while other
  hosts commonly emit lowercase. The SSH review path parses recorded text
  directly and never starts `ssh`.
- Source-tested:
  `contracts/validate-fixtures.py --mirror <macOS fixture directory>` —
  6 contracts, 20 cases, and 7 byte-identical JSON files passed.
- Traceability-tested:
  `contracts/validate-inventory.py <tagged macOS checkout>` — 214 macOS
  references resolved; 64 features across 17 areas passed.
- macOS focused test:
  `swift test --filter PortableContractFixtureTests` — 8 tests passed.
- macOS full unit test:
  `swift test` — 291 tests passed. The isolated worktree used the clean tagged
  checkout's existing `libkitty.dylib` through `LIBRARY_PATH` only because
  SwiftPM links the unchanged executable target while running `KitmuxCore`
  tests.
- A first attempt to rebuild `libkitty.dylib` inside the long temporary
  worktree failed before tests: the pinned Python runtime did not have enough
  Mach-O load-command space for that longer checkout path. Moving the worktree
  shorter avoided the length issue, but the interrupted repair had already
  left its ignored local runtime unusable. No runtime artifact from that
  attempt is evidence for this slice.
- GUI-tested: none; the fixture suite is display-free.
- Package-tested and clean-machine-tested: none.
- Phase 3 is no longer blocked by provisional fixtures. Its Linux consumer
  will be implemented against these unchanged expectations when Slice 3.1 is
  explicitly started.

### 2026-07-25 — Slice 2.2C native Wayland client path

- Extended the existing disposable GTK desktop harness rather than adding a
  second host. It starts Weston 14.0.2 with its GL renderer and kiosk shell,
  without Xwayland, then launches the existing GTK host with
  `GDK_BACKEND=wayland`. The host reports `GdkWaylandDisplay`, so the GTK,
  `GtkGLArea`, input, and IME paths are native Wayland even though Weston is
  nested visibly in the dedicated X11 VNC desktop.
- The Wayland run rendered the real libkitty recorder session with Mesa
  llvmpipe, reported exact host-framebuffer sizes `1024x650`, `760x450`, and
  `980x590`, and byte-compared the tracked host GL state after Kitty drawing.
  The recorder child exited on its sentinel and its PID was dead after the
  GTK window closed.
- One ordinary `a` press/release and a held `a` reached GTK as native Wayland
  key events. Every repeat encoded to `61`; the count remains timing-bounded,
  as on X11. The exact recorder stream also contained the IBus result and
  close sentinel: `61(61)+c3a10d1b5b32347e`.
- A real `m17n:t:latn-post` IBus flow produced the visible preedit `a`, updated
  it in place to `á`, committed `c3a1` through the direct-write route, ended
  preedit, and encoded the composition-ending Return exactly once as `0d`.
  The nested compositor needs an explicit `IBUS_ADDRESS` because it owns a
  separate `XDG_RUNTIME_DIR`.
- XTEST is not used against the Wayland client. The existing injector drives
  only Weston's outer X11 window; Weston translates those events into
  `wl_keyboard` input for the native client. This is deliberately recorded as
  display-bound compositor-bridge evidence, not physical libinput/evdev or a
  general Wayland injection API.
- Visible evidence is `kitmux-linux/gtk-wayland-proof.png`.
- Source-tested: `kitmux-linux/scripts/test-headless.sh` — six tests passed in
  6.44 seconds plus the Rust/C header layout check.
- GUI-tested: `kitmux-linux/scripts/test-desktop.sh` — all existing X11,
  keyboard, Compose/layout, and IBus proofs remained green, followed by the
  native Wayland render/resize/GL/input/IME/clean-close gate.
- Not proven by this slice: a physical Wayland desktop or libinput device,
  physical GPUs, fractional/mixed-monitor scaling, WebKitGTK coexistence,
  event fairness, CJK candidate windows, surrounding-text, accessibility,
  selection, clipboard/paste, mouse/wheel, or search.
- Package-tested: none.

### 2026-07-25 — plan audit, licence decision, and Phase 2 reorder

No implementation, build, or runtime claim was made. Reviewed the plan, this
ledger, `NEXT_STEPS.md`, ADRs 0001–0005, the support matrix, the contracts,
and the live Linux source — CMake, the GTK host, the key translation, both
gate scripts, the release tooling, and the source-lock boundary — against the
macOS reference.

Findings and the changes they produced:

- **Licensing was a Phase 8 checklist item and should have been a Phase 0
  architectural constraint.** Kitty is GPL-3.0-only; the host links it
  transitively, so distribution places the whole combined work under GPL-3.0.
  The only architecture that avoids that is an out-of-process engine, which is
  a Phase 4 process-model decision — so the deferral would have silently
  foreclosed the option it was deferring. Decided: the Linux host is
  GPL-3.0-only free software (ADR 0006). `LICENSE` added at the repository
  root. Phase 0 gained Slice 0.5.
- **Phase 2 had its risk ordering backwards.** Slices 2.2C–2.2E — selection,
  clipboard, mouse, search — were the most expensive remaining work and the
  least likely to disqualify GTK, since VTE ships all of it. Slice 2.3 held
  four cheap checks that plausibly could: Wayland, WebKitGTK coexistence,
  fractional scaling, and main-loop fairness. Reordered: 2.2C Wayland, 2.2D
  WebKit probe, 2.2E scaling, 2.2F fairness. The interaction work moved to
  Phase 4 Slice 4.2, to be written once in the chosen host language.
- **The "disposable" spike was already being inherited.** `NEXT_STEPS.md`
  instructed later sub-slices to build on ~1,400 lines of key-translation and
  harness C that two ADRs described as throwaway, and nobody had decided
  whether the Rust host would call or replace it. ADR 0007 splits the spike
  explicitly: the display-free translation and its fixtures are durable and
  stay C behind FFI; `gtk_terminal_host.c` is disposable.
- **The physical-GPU requirement was blocking the toolkit decision on hardware
  access.** llvmpipe proves correctness; a driver proves driver behavior.
  Moved to Phase 6 as a beta obligation.
- **The feature inventory was too coarse to make Phase 9's parity gate
  falsifiable** — 16 rows against a ~32,000-line macOS application, with
  `navigation.hierarchy` covering an entire subsystem. Decomposition is now
  required before Phase 5.
- **Reference drift was unmeasured.** The macOS baseline is frozen at a tag
  while macOS is a live daily driver. Phase 0 gained Slice 0.6, a re-baselining
  ritual to run at every phase boundary.
- **Four reproducibility defects were named and given due dates** in ADR 0008;
  they are listed under current blockers below. No CI was added: the
  repository has no remote, and the desktop gate should not be edited by
  anyone who cannot run it.

### 2026-07-25 — Slice 2.2B Compose, layouts, and input methods

- The host no longer synthesizes committed text from key symbols. A
  `GtkIMMulticontext` sees every press first; `kitmux_translate_gdk_key` takes
  the committed UTF-8 as an argument and never invents it. Releases stay off
  the input-method path: the kitty keyboard protocol requires them to reach
  the terminal, and VTE likewise filters presses only.
- Commit routing mirrors the macOS text-input contract. The first single-scalar
  commit of a key event, with no composition already in progress, is
  re-encoded through `kitty_session_encode_key` so protocol-aware applications
  see the same event vocabulary they see for every other key. A composition
  result, dead key, Compose sequence, or asynchronous engine commit is written
  to the child as UTF-8 instead. The host logs which of the two routes each
  commit took.
- Preedit start/change/end drive an overlay label positioned at libkitty's own
  cursor cell, and the same rectangle is handed to
  `gtk_im_context_set_cursor_location` so an engine's candidate window
  follows the text. Focus loss resets the context and clears the overlay.
- Fixed a correctness problem this slice introduced: a press the input method
  swallowed used to leave a lone release behind, which is malformed under the
  kitty keyboard protocol. The host now withholds a release only when its
  press never reached the terminal as a key event — a press answered by the
  key-encoder route still owes its release.
- Added `ibus-m17n` and a polkit rule to desktop provisioning. XFCE asks
  polkit to create a colord-managed device on every session start; in a
  headless VNC session nobody answers and the dialog floated over the windows
  the gate screenshots as evidence.
- GUI-tested on GTK 4.22.4, X11, Mesa llvmpipe, in two more windowed runs:
  - GTK's own context, with the child byte stream `c3a6 c3a9 c3bc 40` —
    Compose `Multi_key a e` produced a visible `"a"` preedit and committed
    `æ` off the encoder path; a US-international dead key produced a visible
    `"'"` preedit and committed `é`; German `ü` and AltGr+q encoded as
    ordinary keys carrying committed text, the AltGr level correctly reporting
    its base key `q` with no kitty modifier bit.
  - IBus, with the child byte stream `61 c3a1 0d f09f9a80` — the pass-through
    `xkb:us::eng` engine left encoding to the key path; `m17n:t:latn-post`
    produced a real preedit that updated in place from `a` to `á`; Return
    committed `á` off the encoder path and was then encoded once in its own
    right; an emoji reached the child as `f09f9a80`.
- The Slice 2.2A runs now pin `GTK_IM_MODULE=gtk-im-context-simple`. A session
  with `ibus-daemon` already running would otherwise route ordinary typing
  through IBus and change which path encodes it.
- Visible evidence is `kitmux-linux/gtk-preedit-proof.png`: the live IBus
  preedit rendered at the terminal's cursor cell, over the byte the terminal
  had already received, beside the ordinary GTK controls.
- Source-tested: `kitmux-linux/scripts/test-headless.sh` — six tests passed in
  6.32 seconds plus the Rust/C header layout check.
- GUI-tested: `kitmux-linux/scripts/test-desktop.sh` — the Slice 2.1 and 2.2A
  proofs stayed green alongside the two new input-method runs. The key matrix
  grew to 32 events per terminal state.
- Not proven by this slice: selection, clipboard, paste, mouse, wheel, search,
  Wayland, scaling, and physical GPUs. CJK and other conversion engines are
  untested; `m17n:t:latn-post` was chosen because its preedit and commit are
  deterministic, not because it exercises candidate windows.
- Package-tested: none.

### 2026-07-25 — Slice 2.2A deterministic keyboard input and focus

- Added `src/gtk_key_translation.{c,h}`: a display-free translation from GDK
  key vocabulary to the public `kitty_key_event` contract (functional-key
  numbering, kitty's GLFW-fork modifier bits, base-layout key identity with
  the shifted codepoint alongside, and kitty's text-suppression rules), plus
  a held-key tracker that separates press from auto-repeat. Encoding itself
  stays in kitty through `kitty_session_encode_key`, so DECCKM and the
  keyboard-protocol flag stack come from the live session.
- The GTK host now routes focused key events through that path, keeps an
  ordinary `GtkEntry` beside the terminal, reports focus ownership, widget
  bounds, and every translated event with its exact encoded bytes, can run a
  fixture child instead of a login shell, and can close on child exit.
- Added `tests/pty_input_recorder.c`, a raw-mode PTY child that records the
  exact bytes it receives, can emit a fixed escape sequence at startup to put
  the terminal in a known state, and echoes each read back as hex so the
  rendered screen shows what arrived.
- Added `tests/gtk_key_matrix.c`: 30 key events × 3 live terminal states, with
  expectations written from kitty's documented protocol and its pinned
  `key_encoding.c` rather than captured from this host. It asserts the
  translated event metadata, the exact encoded bytes, and the bytes the real
  PTY child read: 71 bytes in the default and DECCKM states, 186 with kitty
  keyboard-protocol flags 15.
- Added `tests/x11_key_injector.c` (XTEST) and `libxtst-dev` to desktop
  provisioning. `xdotool` is unusable for the function keys here: it infers
  modifiers from the shift level where it finds a keysym, and this session's
  XKB map places `XF86Switch_VT_*` on higher levels of the same keycode, so
  `xdotool key F1` injects keycode 64 (Alt_L) before keycode 67 and XFCE
  consumes the result as Alt+F1. `xinput test-xi2 --root` confirmed the
  injected keycodes; flattening the keymap with `xmodmap` did not fix it.
- GUI-tested on GTK 4.22.4, X11, Mesa llvmpipe, via two windowed runs whose
  child byte streams are matched against fixed expectations:
  - default terminal state — `a`, Shift+`b`, Ctrl+`c`, Alt+`d`, Enter, Tab,
    Backspace, Escape, four arrows, and F1 produced
    `61 42 03 1b64 0d 09 7f 1b 1b5b41 1b5b42 1b5b44 1b5b43 1b4f50`; releases
    correctly produced no bytes; a held `a` produced one press and nine real
    GDK auto-repeats, each encoding to `61`.
  - kitty keyboard protocol (`CSI > 15 u`, set by the child at startup) —
    press, repeat, and release each carried distinct bytes end to end, for
    example `a` as `1b5b393775`, `1b5b39373b313a3275`, and
    `1b5b39373b313a3375`.
- Focus transfer is automated: clicking the adjacent `GtkEntry` moves focus,
  the entry receives `xy`, the terminal child receives nothing while it is
  unfocused, clicking back returns focus, and the next key (`z` → `7a`)
  reaches the terminal again. Both runs ended by the fixture exiting on a
  sentinel, and both left their recorded child PID dead.
- Visible evidence is `kitmux-linux/gtk-keyboard-focus-proof.png`: the
  terminal rendering the exact received bytes, the entry holding `xy`, and the
  ordinary GTK controls.
- Source-tested: `kitmux-linux/scripts/test-headless.sh` — six tests passed in
  6.32 seconds plus the Rust/C header layout check.
- GUI-tested: `kitmux-linux/scripts/test-desktop.sh` — the Slice 2.1 render,
  resize, PTY, GL-state, and clean-close proofs stayed green alongside the new
  keyboard matrix and both windowed key runs.
- Not proven by this slice: Compose, dead keys, AltGr, non-US layouts, IME
  preedit/commit, clipboard, paste, mouse, wheel, search, Wayland, scaling,
  and physical GPUs. The host still synthesizes committed text from key
  symbols; Slice 2.2B must replace that with `GtkIMContext`.
- Package-tested: none.

### 2026-07-24 — durable Slice 2.1 handoff

- Added a GitHub-facing project README, a single Linux operations/evidence
  guide, an exact Slice 2.2 sub-slice handoff, and a history-preserving
  monorepo proposal. No migration or GTK functionality was performed.
- Added a pinned headless Lima definition alongside the existing desktop
  definition so both VMs can be recreated without developer-local state.
- Reconciled the plan, feature inventory, support matrix, and ADRs with the
  actual C GTK spike, the pending GTK decision, and the isolated-libpython
  boundary.
- The documentation audit found an ignored 298 MiB local runtime from an older
  builder. It was preserved, not treated as evidence. Fresh build paths and
  the verified approximately 104 MiB clean-gate result are now distinguished
  explicitly.
- Documentation verification passed `git diff --check`, JSON parsing, YAML
  parsing, both `limactl validate` checks, and the local Markdown-link audit.

### 2026-07-24 — GTK Slice 2.1 real terminal surface

- Replaced the clear-color `GtkGLArea` smoke with `kitmux_gtk_host`: one real
  libkitty engine/session, host-owned GTK framebuffer, GLib PTY source, damage
  redraws, title/bell/exit callbacks, framebuffer resize, a normal GTK status
  row and Close control, and an in-window diagnostic overlay.
- The first link exposed a real toolkit collision: libkitty's broad Python
  development runpath shadowed GTK's distro Cairo, HarfBuzz, GLib, and
  xkbcommon. The host now links only an isolated copy of the pinned
  `libpython`; Kitty's full development-library directory never enters the GTK
  process loader path. This boundary is recorded in ADR 0002.
- `kitmux-linux/scripts/test-desktop.sh` built with warnings-as-errors and
  passed on GTK 4.22.4, X11, and Mesa llvmpipe. The live shell pumped 299 PTY
  bytes through GLib, rendered at 900x530, resized to 760x450 and 980x590,
  restored the tracked host GL state after Kitty draw, displayed the missing
  runtime diagnostic in a separate run, and left its recorded child PID dead
  after the automated close.
- Visual evidence is `kitmux-linux/gtk-terminal-host-proof.png`; it shows the
  GTK control row and Kitty-rendered shell prompt/cursor at the final size.
- `kitmux-linux/scripts/test-headless.sh` remained green: six tests passed in
  6.32 seconds plus the Rust/C header layout check.
- GUI-tested: X11 plus software OpenGL only. Wayland, real GPU, input/IME,
  pointer/clipboard, fractional scaling, flood fairness, and adjacent WebKit
  remain required before choosing GTK.
- Package-tested: none. The isolated-libpython development layout is an
  architecture finding, not package evidence.

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
- GUI-tested: no new GUI claim belonged to Slice 1.3; at that checkpoint the
  earlier clear-color GTK/X11 desktop smoke was the current GUI evidence.
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

- The local desktop VM proves a real GTK/libkitty session over X11 and a
  native Wayland client path under nested Weston, both with llvmpipe. It
  includes press/release/repeat routing, focus transfer to an ordinary GTK
  control on X11, Compose, dead keys, AltGr, a non-US layout, and a real IBus
  preedit/commit flow on both display backends. It does not prove performance
  or driver behavior. A physical Wayland desktop/libinput path, physical GPU,
  selection, clipboard/paste, mouse/wheel, search, fractional or mixed-monitor
  scaling, accessibility, and PTY/frame fairness remain untested. The bounded
  WebKitGTK coexistence check passed, but browser product behavior remains
  unimplemented and out of scope for the terminal-first alpha.
- Input-method evidence covers one Latin conversion engine. CJK engines,
  candidate windows, and surrounding-text requests are unproven; IBus already
  warns that the host has no surrounding-text capability.
- The desktop gate changes session-wide state while it runs: X auto-repeat,
  the keyboard layout, and the active IBus engine. It restores auto-repeat and
  the US layout on exit and is meant for the project VNC session, not a
  desktop in use.
- The GUI keyboard runs assert exact bytes per event but bound, rather than
  fix, the number of auto-repeats: X auto-repeat is the only way to produce a
  real GDK repeat event, and its count is timing-dependent. The fixed
  press/repeat/release expectations live in the display-free key matrix.
- The desktop gate disables X auto-repeat for its keyboard runs and restores
  it on exit. It is a project VNC session control, not something to run
  against a desktop in use.
- The current VM is ARM64. Tier-1 x86_64 proof still requires CI, a remote
  machine, or a separate emulated/native environment.
- Shared portable fixtures are frozen and byte-identical in the current macOS
  mirror. Phase 3 is no longer fixture-blocked, but it remains unstarted and
  must begin as its own numbered slice.
- The clean release gate currently proves ARM64 userspaces. Tier-1 x86_64,
  physical-GPU, native package, and clean desktop-install evidence remain
  future gates.
- Fresh clean-gate runtimes were approximately 104 MiB. Ignored local build
  directories may contain older layouts and are not release evidence; always
  build to a new path and run the runtime audit.
- The GTK development host proves the isolated-libpython loader boundary, but
  no release-shaped GUI artifact proves it yet. A global Kitty dependency
  path would shadow GTK's distribution libraries and is forbidden.

### Reproducibility defects (ADR 0008)

These are properties of the tree, not of any one slice. Each has a due date;
none is due today.

- **R1 — the repository cannot be built standalone.**
  `scripts/materialize-reference.sh` requires the private macOS repository at
  `../macos/kitmux` at the baseline tag, and extracts `libkitty/` and
  `patches/` from it. `kitmux-linux/patches/` is empty in this tree. A clone of
  this repository alone fails at the first step. Due at the monorepo
  migration, which is now a Phase 8 prerequisite.
- **R2 — no automated gate.** Nothing runs on commit. The containerized
  headless gate is already hermetic; only the trigger is missing. Due with the
  first Git remote.
- **R3 — the desktop gate is bound to one VM, not to a display.**
  `scripts/test-desktop.sh` requires `KITMUX_VNC_DISPLAY`, noVNC on port 6080,
  and this project's XFCE session, and it mutates session-global X state.
  No contributor can run it, and it cannot run under Xvfb or headless Weston.
  Due before Phase 4. Do not attempt the edit without being able to run the
  gate afterwards.
- **R4 — x86_64 inputs are unpinned.** Both Tier-1 environments are x86_64 and
  `CMakeLists.txt` resolves `linux-64`, but `source-lock.json` records a
  SHA-256 for `linux-arm64.tar.xz` only. x86_64 is not merely untested; it is
  not reproducible. Due with R2.

All evidence in this ledger was produced by one person, by hand, in two
Lima VMs on one Apple-silicon macOS machine. That is adequate for the current
phase and is not adequate for Phase 8 or for a second contributor.

## Next-agent handoff

Continue [Phase 2 in the implementation plan](LINUX_PORT_PLAN.md#slice-22e-prove-scaling)
using the exact sequence in [`NEXT_STEPS.md`](NEXT_STEPS.md).

Phase 2 was reordered on 2026-07-25. Slices 2.2A through 2.2D are closed;
begin with only Slice 2.2E, **scaling**. Prove coordinates, framebuffer size,
cell metrics, and rendered text at 100%, a fractional scale, and 200%, then
apply a scale change to a live session without cell-metric drift or session
loss. Inspect the Kitty render scale against GTK's integer scale APIs before
choosing a harness mechanism; do not infer fractional correctness from an
integer widget scale factor.

Classify every new file as durable or disposable per ADR 0007 before writing
it. Do not begin selection, clipboard, mouse, or search — those are Phase 4
Slice 4.2 now. Do not begin product chrome, the Rust model, browser
functionality, packaging, or repository migration.
