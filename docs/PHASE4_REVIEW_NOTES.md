# Phase 4 review notes — 2026-07-28

Second-pair-of-eyes review of the Slice 4.1 shell and the in-flight Slice 4.2
interaction work. Nothing here was fixed: `rust/app/src/main.rs` was being
written while this was read, so every edit belongs to whoever owns that file.

Basis: uncommitted worktree, `main.rs` at 1500 lines (mtime 23:32),
`gtk_terminal_bridge.c` at 185 lines, CMake and `test-desktop.sh` as diffed
against `6b2d552`. Line numbers will drift; the anchors are quoted so they stay
findable.

## Act on these before the app ships

### 1. Test-only environment variables can disable safety prompts

`rust/app/src/main.rs` — `autopaste_decision` (~967), `autoclose_decision`
(~977), and the `KITMUX_SYNTHETIC_WHEEL` branch in the scroll handler (~1392).

`KITMUX_AUTOPASTE=confirm` skips the unsafe-paste dialog and
`KITMUX_AUTOCLOSE=confirm` skips the running-process dialog. Both are compiled
into the product binary, so in a packaged build any environment that sets one
removes a guard the macOS behavior contract requires — silently, with the
confirmation code still present and looking correct.

ADR 0007 drew this line for the spike: `KITMUX_GTK_*` harness variables were
disposable precisely because they must not follow the code into the product
shell. Same argument applies here.

Suggested: keep the hooks, put them behind `#[cfg(debug_assertions)]` or a
cargo feature the desktop gate enables, so a release build cannot read them.
`KITMUX_INTERACTION_DIAGNOSTICS` is fine as-is — it only adds log lines.

### 2. Editing the C bridge can silently relink nothing

`rust/app/build.rs` — the script emits only
`cargo:rerun-if-env-changed=KITMUX_NATIVE_LIB_DIR`.

Change `src/gtk_terminal_bridge.c`, rebuild, and CMake produces a new
`libkitmux_terminal_bridge.a` — but Cargo sees no changed input, skips the
link, and the binary keeps yesterday's C code. The CMake `DEPENDS` clause
rebuilds the archive; it cannot make Cargo relink. This fails as "my fix did
nothing," which is the expensive kind of failure.

Suggested: emit `cargo:rerun-if-changed` for
`{native}/libkitmux_terminal_bridge.a` and
`{native}/libkitmux_key_translation.a`.

## Worth fixing, not urgent

### 3. Two exported C functions are never called

`src/gtk_terminal_bridge.c` — `kitmux_widget_surface_scale` (~123) and
`kitmux_session_draw_preserving_gl_state` (~131). No Rust caller for either.

**Checked and not a bug:** the unused fractional-scale helper does *not* mean
pointer coordinates are wrong at 150%. `area.scale_factor()` is the correct
input to `terminal_cell_scaled`, because the `GtkGLArea` backing buffer is
sized by the integer widget factor — the `1/1`, `1.5/2`, `2/2` sequence
recorded for Slice 2.2E. `gdk_surface_get_scale` would be the wrong number
here. Delete the helper or record why it is kept; do not wire it in.

### 4. `let _ = committed;`

`rust/app/src/main.rs` in the key-pressed handler (~1250). `committed` is read
from `filtering_committed` a few lines earlier only to be discarded. Either the
commit state was meant to affect whether the release is withheld, or this is a
leftover from an earlier shape of that decision.

### 5. Shutdown runs twice and logs twice

`rust/app/src/main.rs` — `connect_close_request` calls `shutdown` (~1429) and
returns `Proceed`; the window then closes, `connect_unrealize` fires, and
`shutdown` runs again (~1487). The second pass finds null pointers and emits a
second `kitmux event=shutdown pid=0 reaped=true`.

The desktop gate asserts against these lines, so a `reaped=true` from the no-op
call can mask a `reaped=false` from the real one. Guard with an already-shut-
down flag, or have the gate assert on the first shutdown line only.

### 6. libkitty's error text is discarded on every startup failure

`rust/app/src/main.rs` — `initialize` fills a 1024-byte error buffer (~335) and
then returns `Err("engine-init")`, `Err("renderer-init")`, or
`Err("session-create")` without ever reading it. The window shows "Terminal
unavailable"; the log carries the stage name and nothing else.

Slice 4.1's own goal is an *actionable* diagnostic, and the actionable part —
libkitty's reason for failing — is the part being thrown away. Note that
`render.init-failure-visible` is also one of the four inventory rows with no
macOS test behind it, so no gate on either platform currently pins this.

### 7. Dangling event-source id

`rust/app/src/main.rs` — `pump_pty` returns `0` when the session is null (~846)
without clearing `terminal.pty_source`. A later `shutdown` then calls
`g_source_remove` on an id GLib has already dropped, which is a runtime
critical. Narrow path, one line.

### 8. Wheel speed is an unnamed constant

`rust/app/src/main.rs` — `-dy * cell_points * 5.0` in the scroll handler
(~1395). Five lines per wheel notch, hardcoded; kitty's own default is three.
Scroll feel is a calibration value, not a constant — name it at minimum, and it
belongs in settings once Slice 4.3 has somewhere to put it.

## Small

- `RenderResult` (~33) and an `unsafe extern "C"` block (~45) live in `main.rs`
  while every other `repr(C)` type and extern declaration lives in `ffi.rs`.
  The layout matches `kitmux_render_result` field for field today — verified —
  but a duplicated struct definition is exactly where a later field reorder
  goes wrong without a compiler error.
- `connect_close_request` returns `Propagation::Stop` when the `RefCell` is
  already borrowed (~1425), so the window cannot be closed for that window of
  time. A hang-shaped failure rather than a crash-shaped one.
- `kitmux-linux/CMakeLists.txt` installs `libkitty.so` to both `lib/` and
  `lib/app/` when `KITMUX_BUILD_GTK_HOST` and `KITMUX_BUILD_APP` are both on,
  which muddies the app-only loader isolation the slice claims.
- `build.rs` panics without `KITMUX_NATIVE_LIB_DIR`, so `cargo build` alone
  cannot build the app. Acceptable today; it is one more thing ADR 0008 R1
  (standalone buildability) will have to answer.

## Verified good — do not "fix" these

- GL state capture and restore covers the current program, VAO, both buffer
  bindings, both framebuffer bindings, the renderbuffer, four texture units
  with the active unit restored afterwards, viewport, scissor box, separate RGB
  and alpha blend function and equation, five capability flags, and the depth
  and colour masks. This is the part that is usually wrong and it is not.
- Key releases are withheld only when the press never reached the terminal as a
  key event, and a press answered by the encoder route still sends its release.
  That matches the Slice 2.2B contract exactly.
- Shortcut defaults are Ctrl+Shift for copy, paste, search, and clear
  scrollback, so terminal control codes are not stolen. That was the explicit
  requirement for this slice.
- Diagnostics record byte counts, status codes, and validity flags — never
  clipboard contents, titles, or paths. Titles are stripped of control
  characters and bounded at 256 before reaching the window.
- The PTY source runs at priority 200, matching the Slice 2.2F fairness result.
- The cargo invocation uses `--locked` and `--remap-path-prefix`, and the app
  `Cargo.lock` has a hash in `source-lock.json`, satisfying ADR 0008's rule
  that every new pinned input gets a hash in the same commit.
- `scripts/test-desktop.sh` genuinely closes R3: it accepts a caller-supplied
  `DISPLAY`, makes the noVNC probe optional, and restores keyboard repeat state
  and rate, the complete XKB configuration, and the active IBus engine rather
  than resetting the session to `us`.
