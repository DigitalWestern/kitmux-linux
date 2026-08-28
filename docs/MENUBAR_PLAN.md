# Menu bar plan

Status: approved Linux product scope. Read
[`NEXT_STEPS.md`](../NEXT_STEPS.md) and the scope conflict in section 0
before starting any slice here.

## 0. Scope decision required first

`contracts/feature-inventory.json` now gives the approved Linux menu bar its
own `desktop.menu-bar` area. The remaining `macos-only.appkit-surfaces` row
does not classify Linux menu bars as omitted. `NEXT_STEPS.md` remains the
release-evidence owner; this product slice does not replace the physical-GPU,
remote-CI, x86_64, signing, or vulnerability gates.

macOS Kitmux has no File/Edit/View/Window/Help. `main.swift` builds only an
app menu: About, Hide, Hide Others, Show All, Quit. A Linux menu bar is
therefore new product surface, not parity work, and it makes Linux richer
than macOS in this area.

The settled decisions are:

1. The terminal-only beta admits this Linux-first menu bar.
2. `desktop.menu-bar` contains rows for the bar and File/Edit/View/Window/Help.
3. It is a beta surface, but its product gate runs independently of the
   queued physical-GPU, remote-CI, x86_64, signing, and vulnerability gates.
4. The bar is visible by default and persists through `ValidatedSettings`.
5. Help opens the existing Kitmux website; Report an Issue opens its public
   issue destination. A future bundled-docs link can replace that URL when
   release packaging publishes one.

## 1. What already exists that the menu bar reuses

- `rust/model/src/commands.rs` — `CommandId` (49 stable IDs), `SemanticAction`,
  `command_palette_matches`. Deterministic order, asserted at
  `rust/model/tests/contracts_tests.rs:557`.
- `rust/model/src/interaction.rs:139` — `ShortcutMap::linux_default_bindings`
  maps chords to the same `CommandId`s. Accelerator labels derive from this;
  do not hand-write accelerator strings in the menu model.
- `rust/app/src/menu.rs` — `palette_command_supported` already knows the
  commands that are dead on Linux (`browser.new-pane`,
  `notification.jump-unread`, `app.install-command-line-tool`,
  `app.reload-kitty-config`). Menus reuse it to disable, not hide.
- `rust/app/src/terminal.rs` — `navigation_action` dispatch, with a
  `_ => NavigationEffect::Rejected` catch-all.
- `contracts/fixtures/v1/command-identifiers.json` — the portable catalog
  fixture both platforms consume.

The menu bar adds no new command semantics for roughly 90% of its items. It
is a second view over the existing catalog.

## 2. Platform constraints

- GTK 4 has no global menu bar on XFCE or GNOME. Use in-window
  `gtk::PopoverMenuBar` driven by a `gio::Menu` model, installed with
  `Application::set_menubar` plus `ApplicationWindow::set_show_menubar(true)`.
- DBus-exported menus (`org.gtk.Menus`, or `com.canonical.dbusmenu` for KDE
  and Unity global panels) are a separate optional slice. Do not couple it to
  the in-window work.
- Every menu item routes through a `gio::SimpleAction` registered on the
  `Application` or `ApplicationWindow`. Action names are the existing command
  IDs with `.` and `-` normalized; keep the mapping mechanical and testable.
- The current app is a single `ApplicationWindow` (`rust/app/src/window.rs:24`)
  with a single-window persistence snapshot. `File > New Window` and a real
  `Window` menu need multi-window support first. See slice 6.
- The gtk4 crate is pinned `=0.11.4` with feature `v4_22`. Menu APIs are
  available; no dependency change is needed. Do not add one.

## 3. Menu draft

Items marked NEW have no `CommandId` today and require a catalog addition,
which means a fixture update, an inventory status update, and a bump of the
49-ID assertion.

### File

| Item | Command | Notes |
| --- | --- | --- |
| New Terminal Tab | `terminal.new-tab` | existing |
| New Group | `group.new` | existing |
| New Workspace | `workspace.new` | existing |
| New Window | NEW | blocked on slice 6 |
| Split Right | `pane.split-right` | existing |
| Split Down | `pane.split-down` | existing |
| Connect over SSH… | NEW | `ssh.connect` exists as a control method only; a UI command must route through the existing reviewed-profile flow, never a free-text host field that bypasses review |
| Close Pane | `pane.close` | existing; keeps the foreground close-review chain |
| Close Tab | NEW | `tab.close` exists as a control method with no `CommandId` |
| Close Group | `group.close` | existing |
| Close Workspace | `workspace.close` | existing |
| Quit | NEW | must run the same foreground-process review as window close |

### Edit

| Item | Command | Notes |
| --- | --- | --- |
| Copy | `terminal.copy` | existing |
| Paste | `terminal.paste` | existing; keeps bracketed-paste safety review |
| Paste and Match Style | — | not meaningful in a terminal; omit |
| Select All | NEW | selection model exists; no command ID |
| Find… | `terminal.find` | existing, opens the `SearchBar` |
| Find Next / Find Previous | NEW | the search bar has the behavior; no command IDs |
| Clear Scrollback | `terminal.clear-scrollback` | existing |
| Set Resume Command… | `terminal.resume-command` | existing; Slice 6.3 inert-metadata rules apply unchanged |

### View

| Item | Command | Notes |
| --- | --- | --- |
| Toggle Sidebar | `app.toggle-sidebar` | existing |
| Increase / Decrease / Reset Font | `font.increase`, `font.decrease`, `font.reset` | existing |
| Next Tab / Previous Tab | `terminal.next-tab`, `terminal.previous-tab` | existing |
| Next Group / Previous Group | `group.next`, `group.previous` | existing |
| Focus Pane Left/Right/Up/Down | `pane.focus-*` | existing |
| Resize Pane Left/Right/Up/Down | `pane.resize-*` | existing |
| Zoom Pane | NEW | temporarily maximize one pane in its split tree |
| Full Screen | NEW | window-level, not model-level |

### Window

Mostly blocked on slice 6. Until then this menu is a workspace and tab
switcher.

| Item | Command | Notes |
| --- | --- | --- |
| Workspace 1–9 | dynamic | mirrors the existing `Super+1..9` chords |
| Tab 1–9 | dynamic | mirrors the existing `Alt+1..9` chords |
| Rename Workspace / Group / Tab | `workspace.rename`, `group.rename`, `terminal.rename-tab` | existing |
| Jump to Unread | `notification.jump-unread` | present in the catalog, currently unsupported on Linux; render disabled |
| Move Pane Focus | `pane.focus-*` | existing |

Dynamic sections must be rebuilt from the navigation model on change, not
mutated in place. Cap the enumerated entries and label the overflow rather
than silently truncating.

### Help

| Item | Command | Notes |
| --- | --- | --- |
| Kitmux Help | NEW | reuse the existing `open_url` path |
| Keyboard Shortcuts | NEW | render `ShortcutMap` directly; do not duplicate the binding table |
| Command Palette | existing `ShortcutAction::CommandPalette` | needs an action wrapper |
| Install Command Line Tool | `app.install-command-line-tool` | unsupported on Linux today; render disabled |
| Reload Kitty Config | `app.reload-kitty-config` | unsupported on Linux today; render disabled |
| About Kitmux | NEW | version, GPL-3.0-only, and the bundled attribution set under `release/licenses` |
| Report an Issue | NEW | `open_url` |

## 4. Per-agent menus

### The blocker

There is no agent model on Linux. `agent.start`, `agent.list`, `agent.get`,
`agent.update`, `agent.focus`, and `agent.resume` exist as names in
`ControlMethod` but are absent from `IMPLEMENTED_CONTROL_METHODS`, have no
model type, no persistence, and no inventory row. macOS implements all six in
`ControlDispatcher.swift` with providers claude, codex, opencode, gemini, and
copilot, plus hook installation into each provider's own config.

A per-agent menu is therefore not a menu task. It is the agent-surface port
with a menu on top. Sequence it accordingly.

### Design constraints, in priority order

1. **Never let a menu item write arbitrary text to a live terminal.** Slice
   6.3 established that resume metadata is inert: bounded validated command
   text only, every checkbox off by default, identity and cwd rechecked
   immediately before the write, changed identity skipped. Agent menu actions
   such as Resume, New Session, or Compact Context all write to a live
   terminal and must inherit that entire discipline, not a relaxed version.
2. **Agent-contributed menu items must be declarative and allow-listed.** If
   an agent can contribute menu entries over the control socket, the payload
   is an allow-listed command ID plus bounded parameters. Never a shell
   string, never free-form argv. Follow the `ssh.argv-never-shell` and
   `control.cli-mapping` precedents exactly.
3. **A contributed item is untrusted until reviewed.** Reuse the SSH
   review-and-approval pattern: the resolved action is shown for explicit
   review, and any edit clears a prior approval.
4. **Release builds get no environment-controlled approval bypass.** This is
   already true of the resume path; hold the line here.

### Three implementation options, cheapest first

- **(a) Static per-provider submenus from a manifest.** A checked-in table of
  provider to menu entries, each entry an existing command ID or a bounded
  new one. No runtime contribution, no new trust boundary. Ships in one
  slice. Recommended starting point.
- **(b) Focus-sensitive Agent menu.** The Agent menu's contents change with
  the focused pane's provider and state, driven by the ported agent model.
  Needs the agent surface but still no new trust boundary.
- **(c) Runtime contribution over the control socket.** New `agent.menu.*`
  methods. This is the one that needs a threat-model amendment, framing
  bounds, an allow-list, a review dialog, and quarantine on malformed input.
  Do not start it until (a) and (b) ship and the threat model is updated.

## 5. Non-negotiable per-slice gates

Every slice in this plan closes with:

- `cargo fmt --manifest-path kitmux-linux/rust/app/Cargo.toml -- --check`
- Clippy with `-D warnings` on the ARM64 VM with the release native bridge
- `kitmux-linux/scripts/test-model.sh` — unit, contract, interaction, model,
  and persistence suites
- `kitmux-linux/scripts/test-phase5-product.sh` including the accessibility
  gate, which must still emit
  `kitmux event=accessibility_ready roles=true focus=true`
- `python3 contracts/validate-inventory.py` after any inventory edit
- Any command-catalog change updates
  `contracts/fixtures/v1/command-identifiers.json`, the 49-ID assertion at
  `contracts_tests.rs:557`, and the `navigation.command-catalog` status line

Settings that the menu bar persists (menu bar visible, sidebar visible, font
size) belong in `ValidatedSettings` in `rust/model/src/settings.rs` with a
schema handling path and a fixture update in
`contracts/fixtures/v1/settings.json`. Do not invent a second settings store.

## 6. Slice order

Each slice is independently dispatchable unless a dependency is named.

1. **Scope and contract.** Resolve section 0. Add the `desktop.menu-bar` area
   and rows. No code.
2. **Menu skeleton.** `PopoverMenuBar`, action registration, accelerators
   derived from `ShortcutMap`, disabled-item rendering via
   `palette_command_supported`. Wire only commands that already exist. This is
   the largest single win and touches no new semantics.
3. **Catalog additions.** `tab.close`, Select All, Find Next, Find Previous,
   Zoom Pane, Full Screen, Quit, Help, About. Each needs a fixture and
   assertion update. Splittable across agents by menu.
4. **Accessibility.** GTK menus provide AT-SPI menu roles for free, which
   advances the open full-AT-SPI gate. The gate side landed in
   `test-phase5-product.sh` (F10 → Right → Down → Escape must produce
   `menu_keyboard_traversal roles=true focus=true`) and fails
   deterministically against the app; the completion plan is section 8.
5. **Menu-bar visibility setting** with persistence and fixture.
6. **Multi-window.** `ApplicationWindow` per window, per-window navigation
   root, snapshot schema extension, close-review per window. Large. Unblocks
   `File > New Window` and the real `Window` menu.
7. **Agent surface port.** The six `agent.*` methods, a Linux agent model
   type, provider table, state machine, persistence, and inventory rows.
   Prerequisite for anything per-agent.
8. **Per-agent menus, option (a).** Static provider manifest.
9. **Per-agent menus, option (b).** Focus-sensitive, model-driven.
10. **DBus global menu export.** Optional. KDE and Unity panels.
11. **Runtime agent contribution, option (c).** Only after a threat-model
    amendment.

## 7. Parallel dispatch notes

Independent, no shared files, safe to run concurrently today:

- Finding 1 — implement or explicitly deprecate the 12 unimplemented control
  methods, and reconcile `IMPLEMENTED_CONTROL_METHODS` with `ControlMethod::ALL`
- Finding 2 — add `agent.*` and `todo.*` inventory rows or deferral rows
- Finding 3 — keep the 49-command catalog and contract fixture in sync
- Slice 1 — the scope and contract decision
- Slice 4 — the accessibility gate extension, once slice 2 lands

Do not run slice 6 concurrently with slices 2 or 3; all three touch
`rust/app/src/window.rs` and the navigation root.

## 8. Slice 4 completion plan — the keyboard-traversal gate

Rebuilt 2026-08-27 after the failure was proven deterministic (two
consecutive `test-phase5-product.sh` runs fail at the same point; evidence
in `PORT_STATUS.md`).

### What the gate does and what the app emits

The gate (`test-phase5-product.sh`, `open_menu_via_keyboard`) focuses the
window, presses the live F10 keycode, waits 0.2 s, presses Right, Down,
Escape, then waits for the exact log line
`kitmux event=menu_keyboard_traversal roles=true focus=true`. The app emits
`menu_key key=F10` (the global `ShortcutController` fires) but never the
arrow or Escape diagnostics, so the traversal line never appears.

### Root cause hypothesis, to be confirmed by Task 1

`attach_menu_key_diagnostics` controllers hang on the window, the GL area,
and the menu bar, and the F10 handler additionally walks
`gtk::Window::list_toplevels()` after 50 ms. None of those can see keys
typed into an open menu: a `GtkPopoverMenu` implements `GtkNative` with its
own surface, so its key events dispatch from the popover itself — ancestor
capture controllers never run, and popovers are not `GtkWindow`s, so the
`list_toplevels()` walk never finds them. The fix is to attach the existing
diagnostics controller directly to each popover.

### Task 1: instrument and confirm the delivery path

**Files:** none committed — a temporary patch in
`kitmux-linux/rust/app/src/window.rs`.

- [ ] **Step 1:** Start the desktop VM and session:

```sh
limactl start kitmux-linux-desktop
limactl shell kitmux-linux-desktop -- \
  "$PWD/kitmux-linux/scripts/start-desktop.sh"
```

- [ ] **Step 2:** Temporarily add, after `menu_bar` is constructed in
  `build_window`, a recursive walk that logs every descendant type name and
  attaches a capture-phase key logger to each `PopoverMenu` found:

```rust
fn log_menu_tree(widget: &gtk::Widget, depth: usize) {
    diagnostic(
        "menu_tree",
        &[("depth", depth.to_string()), ("type", widget.type_().name().to_string())],
    );
    let mut child = widget.first_child();
    while let Some(current) = child {
        log_menu_tree(&current, depth + 1);
        child = current.next_sibling();
    }
}
log_menu_tree(menu_bar.upcast_ref(), 0);
```

- [ ] **Step 3:** Build and drive it exactly the way the gate does:

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 bash -c '
  set -e
  runtime=$(mktemp -d)
  KITMUX_BUILD_APP_RUNTIME=1 KITMUX_APP_TEST_HOOKS=ON \
    "$PWD/kitmux-linux/scripts/build-release-runtime.sh" "$runtime/r"
  "$runtime/r/bin/kitmux" >/tmp/menu-probe.log 2>&1 &
  sleep 3
  window=$(xdotool search --name Kitmux | head -1)
  xdotool windowactivate --sync $window
  f10=$(xmodmap -pk | awk '"'"'$2 == "0xffc7" { print $1 }'"'"')
  xdotool key --clearmodifiers $f10; sleep 0.2
  xdotool key --clearmodifiers Right Down Escape; sleep 1
  grep -E "menu_tree|menu_key|menu_keyboard" /tmp/menu-probe.log'
```

Expected: the `menu_tree` lines show whether `GtkPopoverMenu` children
exist at construction time, and the probe shows which widget (if any)
receives Right/Down/Escape. Record both facts in this section before
moving on. If popovers are created lazily (no `GtkPopoverMenu` in the
tree at build time), Task 2 attaches inside the F10 handler instead, with
a run-once guard.

- [ ] **Step 4:** Revert the temporary patch.

### Task 2: attach the diagnostics to the popovers

**Files:** Modify `kitmux-linux/rust/app/src/window.rs` (the
`build_window` menu section and the F10 `CallbackAction`).

- [ ] **Step 1:** Add the walk (final form, not the probe), reusing the
  existing `attach_menu_key_diagnostics`:

```rust
fn attach_popover_diagnostics(widget: &gtk::Widget, menu_navigation: &Rc<Cell<bool>>) {
    if widget.is::<gtk::PopoverMenu>() {
        let popover_keys = gtk::EventControllerKey::new();
        attach_menu_key_diagnostics(&popover_keys, menu_navigation.clone());
        widget.add_controller(popover_keys);
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        attach_popover_diagnostics(&current, menu_navigation);
        child = current.next_sibling();
    }
}
```

Call it once after the menu bar is added to the window
(`attach_popover_diagnostics(menu_bar.upcast_ref(), &menu_navigation);`)
if Task 1 showed the popovers exist at build time; otherwise call it from
the F10 handler behind a `Rc<Cell<bool>>` attached-once guard, replacing
the 50 ms `list_toplevels()` walk.

- [ ] **Step 2:** Delete the `glib::timeout_add_local_once` +
  `list_toplevels()` block from the F10 handler — Task 1 will have shown it
  attaches to nothing useful; keeping it re-adds controllers on every press.
- [ ] **Step 3:** `cargo fmt` and host typecheck (macOS, no GTK install —
  the overrides are recorded in `PORT_STATUS.md` 2026-08-27):
  clippy must stay clean with `-D warnings`.

### Task 3: prove it with the real gate, twice

- [ ] **Step 1:**

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase5-product.sh"
```

Expected: the previously failing wait —
`Kitmux never reported F10, arrow, and Escape menu traversal` — is gone and
the gate passes end to end.

- [ ] **Step 2:** Run it a second time. Expected: pass again — the failure
  was deterministic, so the fix must be too.
- [ ] **Step 3:** Regression sweep, per section 5:
  `test-phase5-navigation.sh`, `test-phase6-control.sh`,
  `test-phase6-resume.sh`, `test-phase6-ssh.sh`, `test-phase4.sh`, and
  `scripts/test-model.sh` in the headless VM. Expected: all pass; the
  accessibility gate still emits
  `accessibility_ready roles=true focus=true`.

### Task 4: record and close

- [ ] **Step 1:** Dated `PORT_STATUS.md` entry: the root cause (popover
  surfaces bypass ancestor capture), the fix, and both gate results.
- [ ] **Step 2:** Remove the traversal item from `NEXT_STEPS.md`, mark
  slice 4 closed in section 6 above, and update the `PORT_STATUS.md`
  header's "keyboard-traversal gate check is still open" clause.
- [ ] **Step 3:** Commit:
  `fix: route menu key diagnostics through the popover surfaces`.

### Fallback if Task 1 falsifies the hypothesis

If the popover controllers still see nothing, stop patching key plumbing
and make the diagnostics semantic instead: connect `closed` on each
`PopoverMenu` and emit `menu_keyboard_traversal roles=true focus=true`
when a popover closes while `menu_navigation` is set and the closing event
was an Escape seen by any diagnostic controller. A menu that opened and
closed under keyboard control is stronger traversal evidence than sniffed
keystrokes; adjust the gate's arrow expectations only if the app-side
events genuinely cannot be observed, and record the reasoning in
`PORT_STATUS.md` either way.
