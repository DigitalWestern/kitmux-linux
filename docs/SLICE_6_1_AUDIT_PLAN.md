# Kitmux Linux — audit remediation plan (Slice 6.1: secure local control and CLI)

**Written:** 2026-08-02
**Repo:** `/Users/ethanabbate/Desktop/System/home-kitmux/operating-system/linux`
**Branch:** `main`
**Audited state:** the **uncommitted** Slice 6.1 worktree on top of `8ae916f`
(`docs: record final slices 0-5 audit verification`). At the time of the audit the
Slice 6.1 work was staged in the working tree, not committed.
**Audience:** an implementing agent. Follow this literally. Do not improvise.

---

## 0. What is true right now (verified by running tools, not by reading docs)

| Fact | Value | How it was checked |
| --- | --- | --- |
| Model tests | **53 pass, 0 fail** | `kitmux-linux/scripts/test-model.sh` on the macOS host |
| Fixtures | 6 contracts / 20 cases / 7 versioned JSON files | `python3 contracts/validate-fixtures.py` |
| Inventory | 234 macOS refs / 64 features / 17 areas / 54 terminal-alpha | `python3 contracts/validate-inventory.py` |
| Control methods | 44 in `control_methods!` | `rust/model/src/control.rs` |
| Lima VMs | `kitmux-linux` and `kitmux-linux-desktop` both **Running** | `limactl list` |
| Slice 6.1 code | uncommitted: 5 new files, 15 modified | `git status --short` |

New files under audit:

```
kitmux-linux/rust/model/src/control_socket.rs      455 lines
kitmux-linux/rust/model/src/cli.rs                 293 lines
kitmux-linux/rust/model/src/bin/kitmuxctl.rs        58 lines
kitmux-linux/scripts/install-user-cli.sh            16 lines
kitmux-linux/scripts/test-phase6-control.sh        165 lines
kitmux-linux/rust/app/src/main.rs                 +719 lines (control dispatch)
```

**Verdict:** the socket *plumbing* is solid — path validation, `0600` mode, `SO_PEERCRED`,
frame bounds, and stale replacement are all real and correctly ordered. The *dispatch layer
in the app* is where the defects are, and the **pane-targeting surface (`pane.send`,
`pane.send_key`, `pane.read_screen`, `pane.focus`) is functionally broken and completely
untested**. The gate script also claims more coverage than it has, and `PORT_STATUS.md`
repeats those claims.

---

## 1. Ground rules — violating any of these fails the task

1. **One task per commit.** Do not batch. Task numbers below are commit order.
2. **Do not start Slice 6.2.** No SSH resolution, no agent workflows, no resume, no
   packaging, no browser, no monorepo migration. This plan closes Slice 6.1 only.
3. **Do not touch `../macos/kitmux`.** Read-only, tagged, clean.
4. **Do not edit `source-lock.json`.** If a hash moves, stop and report.
5. **Never claim a gate passed that you did not run.** Write "not run" in `PORT_STATUS.md`.
   A false evidence line is worse than a missing one. This plan exists because that rule
   was bent once already (Task 16).
6. **App changes cannot be built on macOS.** `rust/app/build.rs` needs
   `KITMUX_NATIVE_LIB_DIR`, GTK 4, and libkitty. All app verification happens in
   `kitmux-linux-desktop`. Model-crate changes (Tasks 8–13, 15) *do* build and test on the
   macOS host — do those first, they are the cheap feedback loop.
7. **Prefer deletion and reuse over new machinery.** Several fixes below are "call the
   function the rest of the app already calls" — do exactly that, do not invent a parallel
   path. One fix is "delete a field".
8. **Preserve unrelated worktree changes.** `git status --short` before you start.

---

## 2. Preflight (Task 0)

```sh
cd /Users/ethanabbate/Desktop/System/home-kitmux/operating-system/linux
git status --short                      # expect: the Slice 6.1 files, nothing else unexpected
python3 contracts/validate-fixtures.py  # expect: 6 contracts, 20 cases, 7 files
python3 contracts/validate-inventory.py # expect: 234 / 64 features / inventory OK
kitmux-linux/scripts/test-model.sh      # expect: 53 passed, 0 failed
limactl list                            # expect: both VMs Running
```

If the Slice 6.1 work is still uncommitted when you start, **commit it as-is first**
(`feat: add Linux secure local control socket and kitmuxctl`) so that each remediation
task below is a reviewable diff against a real baseline. Do not fold fixes into that
commit.

Desktop VM gate invocation, used throughout:

```sh
limactl shell kitmux-linux-desktop -- bash -lc \
  'cd /path/to/repo && DISPLAY=:1 kitmux-linux/scripts/test-phase6-control.sh'
```

---

# PART A — correctness defects in the app dispatch layer

**These need the desktop VM. They are the reason this plan exists.**

---

## Task 1 — `pane.send`, `pane.send_key`, and `pane.read_screen` operate on the wrong pane

**Severity: critical. Silent wrong-target writes into a user's shell.**

### Evidence

`Terminal` derefs to the `SessionState` of `active_surface_id`:

```rust
// rust/app/src/main.rs:579
impl Deref for Terminal {
    type Target = SessionState;
    fn deref(&self) -> &Self::Target { &self.sessions[&self.active_surface_id] }
}
```

`active_surface_id` is assigned in exactly one place — `reconcile_sessions()`
(`main.rs:2980`), which recomputes it from
`navigation.runtime_presentations().find(|p| p.accepts_input)`.

`select_pane()` (`main.rs:1264`) calls `navigation.focus_pane(id)`, which moves the model's
focus — including `active_workspace_index`, `active_group_index`, and `active_tab_index`
(`rust/model/src/model.rs:658`). It does **not** touch `active_surface_id`.

`control_send` (`main.rs:1101`), `control_send_key` (`main.rs:1117`), and
`control_read_screen` (`main.rs:1156`) each call `select_pane(...)` and then immediately
`terminal.borrow()` / `borrow_mut()` and dereference — hitting the **old**
`active_surface_id`'s session. None of the three calls `reconcile_sessions`.

### Consequence

`kitmuxctl pane send <other-pane-id> "text"` types into whichever pane was focused
*before* the call, while silently relocating the user's active workspace/group/tab to the
requested pane. `pane.read_screen <id>` returns the previous pane's screen. This is a
wrong-target write into a live shell, and the response reports success.

### Fix

Route every control mutation through the path the rest of the app already uses. Do **not**
add a new reconcile helper.

- After a successful `select_pane`/`select_tab`/`select_group`/`select_workspace` used for
  targeting, call `apply_navigation_effect(terminal, NavigationEffect::Changed)` — it
  already calls `reconcile_sessions` then refreshes (`main.rs:2994`). Replace the ad-hoc
  `reconcile_sessions(...) + refresh_navigation(...)` pairs and the lone
  `refresh_navigation(...)` calls in `control_select`, `control_rename`, `control_move`,
  `control_close`, and `ControlMethod::PaneFocus` with that single call.
- In `control_send`, `control_send_key`, `control_read_screen`: re-target, apply the
  navigation effect, **then** take the borrow and touch the session.
- Add a debug assertion (or an explicit `not_ready` failure) that the resolved
  `active_surface_id` corresponds to the requested pane before writing bytes. If it does
  not, fail with `not_ready` rather than writing to the wrong pane.

### Acceptance

New gate assertions (Task 16) prove: split into two panes, `pane send <pane-B-id> "x"`,
`pane read-screen <pane-B-id>` shows `x`, `pane read-screen <pane-A-id>` does not.

---

## Task 2 — `pane.focus` changes the UI chrome but not where keystrokes go

**Severity: critical. Same root cause as Task 1, separate user-visible symptom.**

### Evidence

```rust
// rust/app/src/main.rs:784
ControlMethod::PaneFocus => {
    let changed = select_pane(terminal, id);
    if changed { refresh_navigation(terminal); ... }
}
```

`refresh_navigation` (`main.rs:2825`) rebuilds the sidebar, tab strip, group label, and
window title. It does not reconcile sessions. So the window title and sidebar move to the
new pane while `active_surface_id`, the rendered GL surface, and keyboard routing stay on
the old one.

### Fix

Covered by Task 1's change (`apply_navigation_effect`). Keep it a separate commit only if
Task 1's diff would otherwise be hard to review; otherwise fold it into Task 1 and say so
in the commit message.

### Acceptance

Gate: `pane focus <id>` then `pane read-screen current` returns the newly focused pane's
screen; `kitmux event=navigation_changed` reports the new focused pane id.

---

## Task 3 — read-only control operations steal the user's focus and never restore it

**Severity: high. Design defect, not a typo.**

### Evidence

Targeting is implemented as "mutate global selection, then act." `focus_pane`
(`model.rs:658`) reassigns `active_workspace_index`, `active_group_index`, and
`active_tab_index`. So:

- `kitmuxctl pane read-screen <id>` — a read — yanks the user's visible workspace to that
  pane.
- `control_close` (`main.rs:1004`) selects the target **before** the foreground-process
  check at `main.rs:1029`. A close that is then refused with `confirmation_required`
  leaves the user sitting in a workspace they did not choose.
- `control_rename` (`main.rs:893`) selects before validating the name; an
  `invalid_params` rejection still moved the user.

An agent polling `pane.read_screen` on a background pane makes the workspace unusable.

### Fix

Two acceptable shapes; pick the lazy one that holds:

1. **Preferred:** give the model a non-mutating lookup for read paths — resolve
   `PaneId → SurfaceId` via `navigation.runtime_presentations()` (already returns
   `location.surface_id` per pane, `model.rs:731`) and read
   `terminal.sessions[&surface]` directly, without focusing anything. `pane.read_screen`
   and `pane.notify` then never mutate.
2. For write paths that genuinely need focus semantics (`pane.send`, `pane.send_key`),
   either target the resolved surface the same way, or — if the terminal write path truly
   requires the active session — save and restore the prior focus around the operation and
   document that in a `ponytail:` comment naming the ceiling.

Also: in `control_close` and `control_rename`, **validate first, target second.** No
control operation may leave the user relocated after returning a failure.

### Acceptance

Gate: with pane A focused, run `pane read-screen <pane-B>`, then assert the app's
`navigation_changed` diagnostic did **not** fire and `pane read-screen current` still
returns pane A. Same assertion after a `close` refused with `confirmation_required`.

---

## Task 4 — `tab.close` checks the wrong scope for running processes

**Severity: high. Silent process kill; the confirmation exists specifically to prevent this.**

### Evidence

```rust
// rust/app/src/main.rs:764
ControlMethod::TabClose => control_close(terminal, &request, "tab", CommandId::PaneClose),
```

`foreground_surfaces(Some(CommandId::PaneClose))` (`main.rs:2380`) filters to
`presentation.location.pane_id == active_pane` — the focused pane only. Closing a tab with
four panes only checks one of them. `GroupClose` and `WorkspaceClose` pass the correct
command and filter correctly; `tab` is the odd one out.

There is no `CommandId::TabClose` filter arm in `foreground_surfaces` at all — the `Some(_)`
catch-all returns an empty list, so naively passing a tab-scoped command would *disable*
the check entirely. Fix the filter, not just the call site.

### Fix

Add a tab-scoped arm to `foreground_surfaces` matching on
`presentation.location.tab_id == active_tab` and pass that command from `TabClose`. Verify
`CommandId` has a tab-close variant; if it does not, add the filter arm keyed on whatever
command the tab-close UI path already uses — grep `main.rs:3240` and `main.rs:4461` for how
the interactive close path scopes its check and match it exactly.

### Acceptance

Model or gate test: tab with two panes, a long-running process in the **non-focused** pane,
`tab close` returns `confirmation_required`; with `--force` it closes.

---

## Task 5 — `close_confirmed` is stomped, and control mutations run while a modal is open

**Severity: high. Cross-path state corruption.**

### Evidence

```rust
// rust/app/src/main.rs:1038
if force { terminal.borrow_mut().close_confirmed = true; }
...
// rust/app/src/main.rs:1091
terminal.borrow_mut().close_confirmed = false;   // unconditional
```

`close_confirmed` is also set by the interactive close-dialog path (`main.rs:3240`,
`4461`, `4492`). The control dispatch timer runs on the GTK main loop every 10 ms —
including while a modal close dialog's nested loop is running. A control `close` landing
between the user's "Close" click and the model applying it clears `close_confirmed` and
silently cancels the user's confirmed close (or, in the other interleaving, lets an
unconfirmed close through).

`Terminal.close_dialog_open` (`main.rs:466`) exists and is ignored by every control path.

### Fix

- Save and restore `close_confirmed` around the control close instead of clearing it
  unconditionally.
- Reject mutating control methods with a `busy` error while `close_dialog_open` is true.
  Read-only methods (`ping`, `identify`, `capabilities`, `tree`, `event.list`,
  `pane.read_screen` once Task 3 lands) may still proceed.

### Acceptance

Gate: open the close dialog via the existing `KITMUX_AUTOCLOSE` test hook, issue
`kitmuxctl workspace close`, assert a `busy` error and that the dialog's own outcome is
unchanged.

---

## Task 6 — the second instance dies instead of behaving explicitly

**Severity: high. User-visible regression introduced by this slice.**

### Evidence

```rust
// rust/app/src/main.rs:4525
.flags(gio::ApplicationFlags::NON_UNIQUE)
```

```rust
// rust/app/src/main.rs:3919
if let Err(error) = install_control_server(&terminal) {
    diagnostic("control_server_failed", &[("error", error)]);
    app.quit();
    return;
}
```

`NON_UNIQUE` was added so multiple processes may run. `remove_stale_socket`
(`control_socket.rs:337`) returns `LiveServer` when the path is already bound. So the
second instance **always** fails to install the control server and quits — before any
window is shown, writing a diagnostic to stderr that a desktop launcher discards.

Before this slice, launching twice presented the existing window (GTK's default
single-instance behavior). Now it does nothing at all. `LINUX_PORT_PLAN.md:745` calls for
"explicit multiple-instance behavior"; silently exiting is the opposite of explicit.

Any non-`LiveServer` failure — a leftover root-owned socket, a runtime dir with wrong
permissions, an over-long `KITMUX_SOCKET_PATH` — also makes the terminal refuse to start.
The terminal does not need the control socket to be a terminal.

### Fix

Decide and implement one policy, and write it into `LINUX_PORT_PLAN.md` §Slice 6.1:

- **Recommended:** the app starts regardless. On `LiveServer`, log
  `control_server_declined reason=live_server` and run **without** a control server
  (second instance is a normal window, control goes to the first). On any other error, log
  `control_server_failed` and run without control, surfacing the reason in the status line
  so it is diagnosable rather than invisible.
- If the project instead wants one-instance-only, drop `NON_UNIQUE`, use GTK's single
  instance activation, and delete the quit path — but then say so in the plan, because it
  contradicts the flag that was just added.

Whichever is chosen, `install_control_server`'s failure must not be a silent exit.

### Acceptance

Gate: launch instance A, wait for `control_server_ready`; launch instance B; assert B
reaches `navigation_ready`, logs the declined reason, and that `kitmuxctl ping` still
reaches A. Kill A, assert B does not spontaneously acquire the socket (or does, if that is
the chosen policy — assert whichever was written down).

---

## Task 7 — the dispatch loop: two permanent 100 Hz timers, a self-removing GSource, and a dead field

**Severity: medium (efficiency + a latent GLib warning + dead code).**

### Evidence

1. `glib::timeout_add_local(Duration::from_millis(10), ...)` (`main.rs:635`) polls the
   dispatch queue forever, even when no control client has ever connected. Every control
   call also pays up to 10 ms of avoidable latency.
2. `accept_loop` sets the listener non-blocking and `thread::sleep(10ms)` on `WouldBlock`
   (`control_socket.rs:156`). That is a second 100 Hz wakeup. Combined: ~200 wakeups/second
   at idle, forever, on a laptop.
3. `Terminal.control_queue` (`main.rs:481`) is **written twice and never read**. The
   handler closure holds its own `Arc`, so clearing it in `shutdown` (`main.rs:2597`)
   accomplishes nothing.
4. A control `pane.close` on the last pane yields `NavigationEffect::CloseWindow` →
   `apply_navigation_effect` → window close → `Terminal::shutdown` → `control_dispatch_source
   .take().remove()` — removing the GSource **from inside its own callback**, which then
   returns `glib::ControlFlow::Continue`. Expect a `Source ID ... was not found` warning at
   best.

### Fix

1. Replace the polling timer with a wakeup: keep the queue, and have the socket handler
   signal the main loop once per enqueue (`glib::idle_add_once` via a
   `glib::MainContext` invoke from the handler thread, or an eventfd `unix_fd_add_local`).
   Latency drops to ~0 and idle cost drops to zero. If a wakeup mechanism proves awkward
   with the existing `Rc<RefCell<Terminal>>` ownership, keep the timer but raise the
   interval only after measuring — do not silently ship a 100 Hz idle timer without a
   `ponytail:` comment naming the cost.
2. Same for the accept loop: block in `accept()` and unblock on shutdown by connecting to
   the socket once from `Drop` (the identity check already guards the unlink). Delete the
   sleep.
3. Delete the `control_queue` field and both assignments.
4. Make the dispatch callback return `glib::ControlFlow::Break` when it observes that the
   terminal is shutting down, and have `shutdown` skip `source.remove()` if it is the
   currently-executing source — or simpler, have the callback own its own exit by checking
   `weak.upgrade()` (it already does) and let `shutdown` only drop the server.

### Acceptance

- `pane close current --force` on the last pane closes the window with **no** GLib
  warnings in the gate log (`grep -qv 'was not found' "$log"`).
- Idle CPU: with the app up and no client, `pidstat`/`top` in the VM shows the process
  effectively idle. Record the number in `PORT_STATUS.md`.

---

## Task 8 — timeout budgets are not layered; a slow client can hold a slot indefinitely

**Severity: medium-high (same-uid denial of service of the control surface).**

### Evidence

- `CONTROL_IO_TIMEOUT = 2s` (`control_socket.rs:19`) is used for the server's per-read
  timeout, the server's write timeout, **and** the client's read timeout
  (`control_socket.rs:210`).
- The app's dispatch handler waits `recv_timeout(Duration::from_secs(2))` (`main.rs:626`).

So a request that sits 1.9 s in the queue and then answers has already blown the client's
2 s read deadline: the client reports "cannot reach ..." for a request the server
completed. The budgets must nest: client read timeout > handler dispatch timeout > queue
wait.

- `set_read_timeout` is **per read syscall**, not a total deadline. `read_frame`
  (`control_socket.rs:248`) loops. A client sending one byte every 1.9 s holds a connection
  forever. With `CONTROL_MAX_CLIENTS = 32`, thirty-two such connections permanently exhaust
  the accept budget and every subsequent `kitmuxctl` call fails. The gate's "slow client"
  test (`test-phase6-control.sh:104`) opens exactly one idle connection and only asserts
  that `ping` still works — it does not test the cap.
- Over-cap connections are dropped without a response (`control_socket.rs:143`), so the
  client sees `EmptyResponse` → "server closed without a response" instead of a `busy`
  error.

### Fix

- Introduce a total per-connection deadline (`Instant` captured at accept; each loop
  iteration sets the remaining time as the socket read timeout; expire with a
  `timeout` response). Keep `CONTROL_IO_TIMEOUT` as the per-syscall bound.
- Make the client timeout strictly larger than the server's total budget (e.g. server total
  3 s, handler dispatch 2 s, client read 5 s). Name the three constants and comment the
  ordering so a future edit cannot invert it.
- On cap exhaustion, write a `busy` failure frame before closing.

### Acceptance

New Rust integration test (Task 15): 32 idle connections + 1 more → the 33rd receives a
`busy` response, not EOF; a client dribbling bytes is disconnected at the total deadline
and a subsequent `ping` succeeds.

---

# PART B — protocol and model defects

**These build and test on the macOS host. Do them first.**

---

## Task 9 — parameter validation is client-side only, so it is not a boundary

**Severity: high. The security property the slice claims is enforced in the wrong process.**

### Evidence

`cli.rs:250 insert_param` rejects empty keys/values, keys > 128 bytes, values >
`CLI_MAX_ARGUMENT_BYTES` (16 KiB), and any control character. That is the *client*.

`decode_control_request` (`control.rs:194`) validates `version`, `id`, and `method` — and
**nothing about `params`**. Any process with the user's uid can open the socket directly
(the gate's own Python helper does exactly this) and send:

- a `pane.send` `text` of ~64 KiB containing raw escape sequences and `\r`, bypassing both
  the CLI's control-character rule and the app's `paste_confirmation_threshold` /
  `paste_confirmation_reason` machinery that guards interactive pastes;
- arbitrarily many params with arbitrarily long keys.

`Terminal::paste` (`main.rs:2145`) is called directly by `control_send` (`main.rs:1101`)
with no confirmation check.

This matters even for a same-uid attacker model: it is the difference between "an agent
can type into your shell" and "any process that can reach the socket can execute arbitrary
commands in your shell without the confirmation the UI enforces."

### Fix

- Move the bounds into `decode_control_request`: max param count, max key length, max value
  length, no control characters in keys, and a documented control-character policy for
  values (reject by default; if a method genuinely needs `\n`, allow it for that method
  explicitly, not globally).
- Have `control_send` consult `paste_confirmation_reason` with the app's configured
  threshold and return `confirmation_required` unless `force=true` is present — mirroring
  the close path's contract.
- Keep the CLI checks. They stay useful for error messages; they are no longer the boundary.

### Acceptance

- Contract test: a request whose `params` exceed the new bounds is rejected by
  `decode_control_request` with a stable code.
- Gate: raw-socket `pane.send` with 32 KiB of text returns `confirmation_required`.

---

## Task 10 — the event history misses exactly the events worth auditing

**Severity: medium. The audit surface has holes where the security events are.**

### Evidence

`history.record(...)` is called only from the GTK dispatch loop (`main.rs:646`), after a
request has already been decoded, authorized, and queued. Every pre-dispatch rejection is
therefore **absent from the history**:

- peer uid mismatch → `unauthorized` (`control_socket.rs:185`)
- peer credentials unavailable (`control_socket.rs:186`)
- malformed / oversized frames (`control_socket.rs:181`)
- dispatch queue full → `busy` (`main.rs:616`)
- dispatch timeout (`main.rs:626`)

`ControlEvent` (`control_socket.rs:409`) also carries no timestamp and no request id, so
the log cannot be correlated with anything or ordered against other diagnostics.

`event.list` returns `eventCursor` = the cursor of the last **returned** event
(`main.rs:734`). With a `category` filter that matches nothing, that is `0`, so a polling
client using `--after $cursor` re-scans from the beginning forever and can never advance
past non-matching events.

### Fix

- Record rejections where they happen. `ControlEventHistory` is already `Clone` + `Arc`;
  pass a handle into `ControlServer::start` (or into the handler closure) and record
  pre-dispatch outcomes with the method string `"<rejected>"` or the decoded method when
  available.
- Add a monotonic timestamp and the request id to `ControlEvent`.
- Return the history's current maximum cursor from `event.list`, not the last matching
  event's cursor.
- Remove the duplicated `limit.min(500)` (`main.rs:733` and `control_socket.rs:451`) — keep
  the clamp in exactly one place, in the model.

### Acceptance

Gate: send an oversized frame over a raw socket, then `kitmuxctl events` shows a rejection
entry. `kitmuxctl events --category ssh` (matching nothing) returns a cursor that advances.

---

## Task 11 — `pane.read_screen`: mixed units, an ignored parameter, an unbounded encoded response

**Severity: medium.**

### Evidence

```rust
// rust/app/src/main.rs:1156 (control_read_screen)
let total = text.len();                       // bytes
let truncated = total > 256 * 1024;           // bytes
let text = text.chars().take(256 * 1024)...   // characters
"byteCount": text.len(), "totalByteCount": total, "truncated": truncated
```

- `total`/`truncated` are byte counts; the truncation is a character count. For non-ASCII
  screens they disagree, and the response can still exceed the intended cap.
- The CLI accepts `kitmuxctl pane read-screen ID LINES` and sends a `lines` param
  (`cli.rs:204`). **The server never reads it.** The CLI advertises a limit it does not
  apply.
- The 512 KiB response cap (`CONTROL_MAX_RESPONSE_BYTES`) applies to the *JSON-encoded*
  frame. JSON escaping of control characters costs 6 bytes each, so a 256 KiB screen of
  escapes encodes past 512 KiB, `encode_control_response` fails
  (`control_socket.rs:188`), and the client gets a confusing `response_too_large` for an
  ordinary read.

### Fix

- Pick one unit (bytes) and use it for the cap, the count, and the flag.
- Implement `lines` (tail N lines) or remove it from the CLI. Do not ship a parameter that
  is silently dropped.
- Bound the response by *encoded* size: build the JSON, and if it exceeds the cap, shrink
  the text and re-encode (or compute a conservative pre-encode budget). Return `truncated:
  true` rather than an error.

### Acceptance

Contract test on the encoding path with a screen full of control characters; gate asserts a
successful truncated read rather than `response_too_large`.

---

## Task 12 — socket lifecycle hazards in `control_socket.rs` / `platform.rs`

**Severity: medium. Individually small, collectively the difference between "careful" and "proven".**

### 12a — bind race between two starting instances

`remove_stale_socket` (`control_socket.rs:337`) does connect → `remove_file` → then the
caller binds. Two instances starting simultaneously can both observe "no live server";
the second's `remove_file` deletes the **first's freshly bound socket**, then binds its
own. The first keeps listening on an unlinked inode — invisible to every client, and its
`Drop` identity check correctly declines to remove the second's socket, so nothing ever
reports the problem.

**Fix:** take an `O_CREAT|O_EXCL` (or `flock`) lock file in the same private directory
around the check-remove-bind sequence, or bind to a temporary path and `rename` it into
place (atomic replace). Prefer the lock file; it is fewer moving parts and also gives Task
6 a clean `LiveServer` signal.

### 12b — ancestor validation silently skipped on error

```rust
// rust/model/src/platform.rs:116
if let Some(runtime_root) = parent.parent()
    && runtime_root != Path::new("/tmp")
    && let Ok(metadata) = fs::symlink_metadata(runtime_root)
{ validate_private_directory(&metadata, expected_uid)?; }
```

An `Err` from `symlink_metadata` (EACCES, ELOOP) **skips the check** instead of failing.
The `/tmp` special case is also exact-match only and one level deep — the default
`$XDG_RUNTIME_DIR/kitmux/kitmux.sock` gets its grandparent checked, but a user-supplied
`KITMUX_SOCKET_PATH` under a deeper tree does not.

Note the gate never exercises this: `test-phase6-control.sh:22` puts the socket directly in
a `mktemp -d` under `/tmp`, so the grandparent is `/tmp` and the new branch is skipped
entirely. **This code is currently untested.**

**Fix:** treat metadata errors as failures. Decide explicitly how many ancestors are
validated and write it in the doc comment. Add a model test with a world-writable
grandparent.

### 12c — `SIGPIPE` on non-Linux

`write_frame` (`control_socket.rs:305`) passes `MSG_NOSIGNAL` on Linux and `0` elsewhere.
The model crate is built and tested on this macOS host. A client that disconnects mid-write
kills the process with `SIGPIPE` instead of returning `EPIPE`.

**Fix:** set `SO_NOSIGPIPE` on the accepted socket for non-Linux targets, or gate the whole
server module behind `#[cfg(target_os = "linux")]` and accept that the CLI cannot be
smoke-tested on the dev host (say so in `docs/LINUX_DEVELOPMENT.md`).

### 12d — every read error is reported as "malformed"

`read_frame` maps **all** `io::Error`s — including timeouts — to
`ControlCodecError::IncompleteFrame`, whose response code is `malformed_request`
(`control.rs:168`). A user whose client was merely slow is told their request was
malformed. Distinguish timeout from malformed.

### 12e — failure responses lose the request id

`serve_client` (`control_socket.rs:181`, `185`, `186`) and the dispatch timeout path
(`main.rs:627`) build failures with `ControlResponse::failure("", ...)`. Echo the decoded
request id whenever one is available.

### Acceptance

Rust integration tests (Task 15) for 12a and 12b; a model test for 12d's error mapping.

---

## Task 13 — drift-prone duplicated truth: `capabilities.implemented`, versions, socket resolution

**Severity: low-medium. Guaranteed to rot.**

### 13a — `implemented` is a hand-typed list

`main.rs:717` hand-lists 28 method strings that must stay in sync with the dispatch `match`
below it. Nothing enforces the relationship. `pane.rename` is correctly excluded today
(it returns `unsupported_method` at `main.rs:797`) — that will not survive the next edit.

**Fix:** derive it. Put a `const IMPLEMENTED: &[ControlMethod]` next to the dispatch match,
build the JSON from it, and have the fallback arm return `unsupported_method` for anything
not in it. Add a contract test asserting every `IMPLEMENTED` entry is a real
`ControlMethod` and that the count matches.

### 13b — hardcoded version strings

`"version": "0.1.0"` (`main.rs:722`) and `"kitmuxctl 0.1.0"` (`cli.rs:26`). Use
`env!("CARGO_PKG_VERSION")` in both.

### 13c — the CLI and the app can resolve different socket paths

The app resolves `HOME` via `getpwuid_r` with an env fallback (`account()`, `main.rs:158`).
The CLI uses `environment["HOME"]` and falls back to `/tmp` (`cli.rs:76`). With `HOME`
unset or non-absolute — a systemd unit, a cron job, some `sudo` configurations — the two
compute different default socket paths and `kitmuxctl` reports "cannot reach" while Kitmux
is running.

**Fix:** give the model one resolution function used by both, with the `getpwuid_r`
fallback, and have `kitmuxctl` call it.

### 13d — related, worth documenting rather than coding

`$XDG_RUNTIME_DIR` is removed on logout and `/tmp` entries are reaped by
`systemd-tmpfiles`. If the socket file disappears while the server still holds the listening
fd, every client gets `ENOENT` and the CLI's message says "Start Kitmux first" while Kitmux
is running. Do **not** build a watcher for this. Add one sentence to
`kitmux-linux/README.md` telling the user to restart Kitmux or set `KITMUX_SOCKET_PATH`,
and note it as a known limitation in `PORT_STATUS.md`.

---

# PART C — build, tests, and evidence

---

## Task 14 — CMake compiles the model crate twice

**Severity: low (build time and disk only).**

`CMakeLists.txt` gives the CLI its own `CARGO_TARGET_DIR`
(`cargo-cli`) separate from the app's (`cargo-app`). Same crate, same profile, same
`RUSTFLAGS` — so `kitmux-model`, `serde`, `serde_json`, `sha2`, and `uuid` are all compiled
twice per clean build.

**Fix:** point `kitmux_cli` at `${KITMUX_APP_TARGET_DIR}` and take
`${KITMUX_APP_TARGET_DIR}/release/kitmuxctl` as the byproduct. Verify the app's
`--features test-hooks` (applied to the *app* package) does not perturb the shared
dependency graph; if it does, keep the split and leave a comment saying why.

Also reverse the dependency: `add_dependencies(kitmux_app kitmux_cli)` makes the app wait on
the CLI, which is backwards. Both are `ALL` targets; the edge is only ordering noise.

Update `build-release-runtime.sh:103` to install from whichever path wins.

**Acceptance:** a clean `KITMUX_BUILD_APP_RUNTIME=1 build-release-runtime.sh` produces both
binaries and is measurably faster. Record before/after wall-clock in `PORT_STATUS.md`.

---

## Task 15 — 455 lines of security-critical socket code have zero Rust tests

**Severity: high. This is why Tasks 1–3 and 8 shipped undetected.**

Today the only coverage of `control_socket.rs` is the X11 GUI gate, which needs a desktop
VM, a display, a full release runtime build, and libkitty. Nothing about the socket needs
any of that.

**Fix:** add `rust/model/tests/control_socket_tests.rs`, gated
`#![cfg(target_os = "linux")]`, using `ControlServer::start` with a trivial handler and a
`tempdir` under `XDG_RUNTIME_DIR`. Cover:

1. round-trip request/response over a real socket
2. socket mode is `0600`, type is socket, owner is the current uid
3. stale socket (no listener) is replaced; a **live** socket yields `LiveServer`
4. a symlink at the socket path yields `UnsafeExistingPath` and is not followed or removed
5. oversized frame → `request_too_large`; malformed frame → `malformed_request`
6. `CONTROL_MAX_CLIENTS + 1` connections → the last gets `busy` (Task 8)
7. a dribbling client is cut off at the total deadline and the server still serves (Task 8)
8. `Drop` removes the socket only when the inode identity still matches
9. bind race: two `ControlServer::start` calls on the same path — one wins, the other gets
   `LiveServer`, and the winner's socket file survives (Task 12a)
10. `prepare_parent` rejects a world-writable ancestor (Task 12b)

These run in `kitmux-linux` (headless VM) — no desktop, no GTK, no X11. Wire them into
`kitmux-linux/scripts/test-model.sh` guarded by platform so the macOS host still passes.

**Acceptance:** `test-model.sh` reports 53 on macOS and 53 + N on Linux, 0 failures.

---

## Task 16 — the gate script is weaker than it looks, and its failure trap does not fire

**Severity: high (evidence integrity).**

### 16a — `trap ... ERR` without `set -E`

`test-phase6-control.sh:42` installs `trap dump_failure ERR` but the script sets only
`set -euo pipefail`. Without `set -E` (`errtrace`), the ERR trap is **not inherited by shell
functions** — so a failure inside `launch_app`, `cli`, or `build_runtime` exits without ever
dumping the app log, which is the entire point of the trap.

**Fix:** `set -Eeuo pipefail`.

### 16b — duplicated assertion

Lines 138 and 140 are both `[[ -S "$socket_path" ]]`. Delete one.

### 16c — missing coverage that the docs claim exists

The gate currently exercises: socket mode/owner/type, `ping`, `identify`, `tree`,
`workspace new`, `events`, one idle client, malformed and oversized frames, stale
replacement across a restart, and symlink refusal. That is genuinely good. It does **not**
cover:

| Claimed in `PORT_STATUS.md` / `feature-inventory.json` | Actually tested? |
| --- | --- |
| "bounded clients" (`CONTROL_MAX_CLIENTS = 32`) | **No** — one idle connection only |
| "timeouts" | **No** — nothing exceeds any deadline |
| "explicit multiple-instance behavior" | **No** — the `LiveServer` path is never taken |
| "the user-local CLI fallback" | **No** — `install-user-cli.sh` is never invoked |
| default XDG socket path resolution | **No** — `KITMUX_SOCKET_PATH` is always set |
| the new grandparent check in `prepare_parent` | **No** — grandparent is `/tmp`, branch skipped |
| the entire `pane.*` surface | **No** — zero pane assertions |

**Fix:** add gate cases for multiple-instance behavior (Task 6), the pane surface (Tasks
1–3), `install-user-cli.sh` (invoke it against the built `kitmuxctl`, put its bin dir on
`PATH`, run a command through it, then assert the documented diagnostic when the symlink
target is removed), and one launch with `KITMUX_SOCKET_PATH` **unset** so the default XDG
resolution path executes. Client-cap and timeout coverage belongs in Task 15's Rust tests,
not here — say so explicitly in a comment so the next reader does not look for it in the
wrong file.

### 16d — flake risk in the oversized-frame test

`test-phase6-control.sh:128` sends 65537 bytes; the server may respond and close before the
client's `sendall` drains. Wrap the send in a `try/except BrokenPipeError` that still
asserts the response was read, or send the payload in chunks and tolerate `EPIPE` after a
response has arrived.

### 16e — the symlink case encodes behavior Task 6 will change

Lines 150–161 assert the app exits **0** and logs `control_server_failed`. If Task 6 makes
the app start anyway, this assertion inverts. Update it in the same commit as Task 6, not
separately.

---

## Task 17 — correct the evidence claims in `PORT_STATUS.md` and `contracts/feature-inventory.json`

**Severity: high. This is the repo's core discipline and it slipped.**

### Evidence

`PORT_STATUS.md` (Slice 6.1 entry, ~line 46) states the gate proved "bounded
clients/frames/I/O/timeouts" and that `install-user-cli.sh` provides "a diagnosable
user-local fallback." `LINUX_PORT_PLAN.md:740` states the gate "proves … bounded clients and
I/O, … explicit multiple-instance behavior, and the user-local CLI fallback."
`contracts/feature-inventory.json` attributes to
`test-phase6-control.sh::Slice 6.1 secure local control and CLI gate`:

- `control.*`: "Slice 6.1 proves … bounded clients, and timeouts"
- `control.cli-mapping`: "Slice 6.1 proves … the user-local fallback"

Per Task 16c, none of those four are exercised by that script.

### Fix

Two commits:

1. **Immediately (before any code work):** correct the claims to match what the script
   actually runs. Move "bounded clients", "timeouts", "multiple-instance behavior", and
   "user-local fallback" to explicitly unproven. This is the honest state and it must not
   wait for the fixes.
2. **After Tasks 15 and 16:** re-attribute each item to the specific test that now proves
   it (`control_socket_tests.rs::<name>` for the cap and deadlines; the gate for
   multi-instance and the CLI fallback).

Also update the frontmatter: `LINUX_PORT_PLAN.md:10` and `NEXT_STEPS.md` currently say
Slice 6.1 is complete and Slice 6.2 is next. Slice 6.1 is **not** complete until this plan
is closed. Say that.

### Acceptance

`python3 contracts/validate-inventory.py` passes; every `linux_tests` entry names a test
that exists and that you personally ran.

---

## Task 18 — the model crate's own doc comment is now false

**Severity: low, but it is a one-line fix and the comment is load-bearing for reviewers.**

```rust
// rust/model/src/lib.rs:1
//! It deliberately has no GTK, WebKit, libkitty, shell-execution, or
//! network-runtime dependency.
```

`control_socket.rs` adds a threaded Unix-domain socket server to this crate. Whether or not
that counts as a "network runtime" is a judgment call — but the comment as written tells a
reviewer something untrue about what lives here.

**Fix:** either amend the comment to state that the crate owns the local control transport
(and why it is not a network dependency: no sockets beyond `AF_UNIX`, no DNS, no TLS), or
move `control_socket.rs` into the app crate. Prefer amending — the CLI binary needs the
transport and it does not belong in a GTK crate.

---

## 3. Suggested commit order

Model-crate work first (fast macOS feedback), then app work (VM), then evidence.

| # | Task | Where it runs |
| --- | --- | --- |
| 0 | Preflight; commit the Slice 6.1 baseline | macOS |
| 1 | Task 17 step 1 — correct the overclaims **now** | macOS, docs only |
| 2 | Task 9 — server-side param validation | macOS |
| 3 | Task 10 — event history gaps | macOS |
| 4 | Task 11 — read_screen units / `lines` / encoded bound | macOS |
| 5 | Task 12 — socket lifecycle (a–e) | macOS + headless VM |
| 6 | Task 13 — drift: capabilities, versions, socket resolution | macOS |
| 7 | Task 18 — crate doc comment | macOS |
| 8 | Task 15 — Rust control-socket integration tests | headless VM |
| 9 | Task 8 — layered timeouts + `busy` on cap | headless VM |
| 10 | Task 1+2 — pane targeting and focus | desktop VM |
| 11 | Task 3 — read-only ops must not steal focus | desktop VM |
| 12 | Task 4 — tab close scope | desktop VM |
| 13 | Task 5 — `close_confirmed` / modal reentrancy | desktop VM |
| 14 | Task 6 — multiple-instance policy | desktop VM |
| 15 | Task 7 — dispatch loop, dead field, GSource | desktop VM |
| 16 | Task 14 — CMake shared target dir | desktop VM |
| 17 | Task 16 — gate script fixes and new cases | desktop VM |
| 18 | Task 17 step 2 — re-attribute evidence | macOS |

---

## 4. Explicit non-goals

Do not do any of these, even if they look adjacent:

- Slice 6.2 (SSH resolution, `ssh -G`, agent workflows) or 6.3 (resume, recovery).
- The physical Mesa GPU obligation. It is a Phase 6 beta gate, not part of this plan.
- Live macOS import/restore, browser/WebKitGTK work, packaging, or the monorepo migration.
- A general authentication or capability-token scheme for the control socket. Same-uid
  peer credentials plus a `0700` parent directory is the agreed model; do not expand it.
- A file watcher or auto-rebind for a socket deleted underneath a running server (13d) —
  document it, do not build it.
- Human-readable output formatting for `kitmuxctl`. Today `--json` differs from the default
  only by including the envelope; both emit JSON. That is a product decision, not a defect.
  Leave it.

---

## 5. What was checked and found *correct* (do not "fix" these)

Recorded so the next reader does not re-litigate them:

- **Socket creation ordering** — `prepare_parent` → `remove_stale_socket` → `bind` →
  `chmod 0600` → `validate_bound_socket` (dev/ino identity) is correct, and the
  post-bind revalidation closes the obvious substitution window.
- **The bind-to-`chmod` window** where the socket briefly carries umask permissions is
  harmless: the parent directory is enforced `0700` and owner-checked, so nothing else can
  reach it. Do not add a `umask` dance.
- **`Drop` / `accept_loop` unlink guards** correctly compare `(dev, ino)` before removing
  the socket file, so a server never deletes a successor's socket.
- **`decode_control_response`'s consistency rules** (`ok` XOR `error`, `result` present iff
  `ok`) are right and the contract test covers them.
- **`LineFrameDecoder`'s CRLF and over-length handling**, including the `maximum + 1`
  trailing-`\r` allowance, is correct.
- **The CLI never builds a shell string.** `pane send` passes text as a param and the
  contract test asserts an unknown method like `"shell -c rm"` is rejected. This is the
  right shape and the test is well chosen.
- **Peer credentials are checked before decoding**, so an unauthorized peer never reaches
  the parser.
- **`install-user-cli.sh`** resolves through `realpath` and uses `ln -sfn`; it is small and
  correct. Its only problem is that nothing runs it (Task 16c).
