# libkitty

An embeddable terminal engine extracted from [kitty](https://github.com/kovidgoyal/kitty),
for hosts that want kitty's terminal emulation and GL rendering inside windows
they own — without kitty's app shell (GLFW, Boss, OS windows, main loop).

**Architecture (v0.20):** libkitty embeds a CPython interpreter in the host
process and drives kitty's C core (`Screen`, parser, fonts, `draw_cells`)
through it. The host process, run loop, threads, and GL contexts all belong to
the host; the interpreter is a passive subsystem entered per-call under the
GIL. Kitty is vendored at a pinned commit with one additive patch
(`patches/0001-libkitty-render-exports.patch`, +199 lines) exposing render
entry points. Incremental de-Pythonization is a later phase, behind this same
API. Evidence for all of this lives in `../docs/` (event-loop findings,
renderer extraction notes, python bundling notes).

**License:** GPLv3, same as kitty.

## Build and test

```sh
../scripts/build-kitty.sh   # once: vendored kitty + patch + fast_data_types
make test                   # engine lifecycle, session API, offscreen render
make run-example            # AppKit window running your shell via libkitty
make dist                   # dist/: libkitty.dylib + libkitty.h + py/glue.py
```

## The v0.21 API

The complete surface is in [`include/libkitty.h`](include/libkitty.h) —
a deliberately small, additive API. In plain English:

**Engine** (one per process — it owns the embedded interpreter):

- `kitty_engine_init(cfg, errbuf, len)` — start the interpreter, import
  kitty, load config. `cfg` says where the kitty checkout and glue.py live,
  optionally a bundled Python runtime (`python_home`) and a `kitty.conf`.
- `kitty_engine_shutdown(e)` — finalize the interpreter and free everything.
- `kitty_engine_config_number(e, key, &out)` — read a parsed numeric option
  from kitty's real config (v0.13, additive); colors are returned as packed
  `0xRRGGBB`. Unknown or non-numeric keys return false. The embedding host
  applies values for pixels and behavior it owns.
- `kitty_engine_config_string(e, key)` — read a parsed UTF-8 string option
  as a caller-freed allocation (v0.16, additive). S5c uses it for Kitty's
  `bell_on_tab` marker without duplicating Kitty's parser.
- `kitty_engine_reload_config(e, path, errbuf, len)` — re-read Kitty config
  and patch live session colors (v0.15, additive); failure preserves the last
  good configuration.

**Sessions** (a kitty `Screen` + a child process on a PTY):

- `kitty_session_create(e, lines, cols, argv, cbs, errbuf, len)` — fork a
  child on a new PTY (`argv` NULL = login shell) and parse its output into a
  kitty screen. `cbs` registers optional callbacks: `on_damage` (screen
  changed — schedule a redraw), `on_title`, `on_bell`, `on_child_exit`, and
  `on_notification` (v0.9, additive) — one call per OSC 9/99/777 desktop
  notification the child emits, with control-stripped UTF-8 title/body.
  OSC 777 requires the `notify;` subcommand (kitty parity); OSC 99 gets a
  pragmatic single-part subset (`p=title`/`p=body`, `e=1` base64) with
  chunked commands, ids, actions, and queries dropped quietly. Bells stay
  separate: BEL rings `on_bell`, never a notification. `on_user_var`
  (v0.11, additive) — one call per OSC 1337 `SetUserVar` the child emits,
  with the key and base64-decoded UTF-8 value (empty value = cleared).
  libkitty forwards every user var generically; the host decides which keys
  it acts on.
- `kitty_session_has_foreground_process(s)` — report whether the live PTY has
  a foreground process group distinct from its shell (v0.17, additive).
  Previously observed stopped foreground jobs remain true; running background
  jobs, idle shells, and exited children are false.
- `kitty_session_create_with_cwd(...)` — the same fresh session, with an
  optional child launch directory and safe fallback when it is unavailable.
- `kitty_session_create_with_options(...)` — additive v0.19 creation with the
  same optional cwd plus a NULL-terminated `KEY=VALUE` environment override
  list. Overrides are applied only to the new child after Kitty's normal
  environment and shell-integration setup; Kitmux uses it for pane identity.
- `kitty_session_fd(s)` — the master PTY fd. Watch it with your run loop
  (kqueue/`DispatchSource`/`CFFileDescriptor`) and pump when readable.
- `kitty_session_pump(s)` — read pending child output, parse it, flush the
  terminal's reply bytes back to the child, fire callbacks. Non-blocking.
  Callbacks arrive on the calling thread, outside the GIL.
- `kitty_session_write(s, data, len)` — send input bytes to the child.
- `kitty_session_paste(s, data, len)` — clipboard paste with kitty's
  semantics (v0.3, additive): wraps with `ESC[200~`/`201~` and strips
  embedded end markers when bracketed-paste mode (2004) is set, normalizes
  newlines to `\r` when not.
- `kitty_session_scroll(s, lines)` — scroll the kitty-owned history viewport
  (v0.4, additive; positive is up). On an alternate screen such as vim or
  less, sends encoded Up/Down keys instead, matching kitty's wheel behavior.
- `kitty_session_clear_scrollback(s)` — clear this session's history and
  return its viewport to the live bottom without clearing the live screen.
- `kitty_session_scrolled_by(s)` — inspect the current viewport offset in
  lines (`0` means live bottom).
- `kitty_session_selection_start(s, column, row, in_left_half, extend_mode)` —
  begin a non-rectangular cell, word, or line selection at a visible viewport
  coordinate (word/line modes added in v0.12).
- `kitty_session_selection_update(s, column, row, in_left_half, ended)` —
  update or finish the current selection.
- `kitty_session_selection_clear(s)` — clear this session's selection.
- `kitty_session_selection_text(s)` — return selected plain UTF-8 as a
  caller-freed allocation; empty when there is no non-empty selection.
- `kitty_session_search_set(s, query, len)` — install a literal,
  case-insensitive focused-session search across the live main screen and
  kitty history (v0.10, additive), render every match through kitty's native
  mark colors, automatically reveal a match, and return the occurrence count.
- `kitty_session_search_set_options(...)` — additive v0.18 case-sensitive and
  bounded regex search. Invalid syntax, patterns over 512 UTF-8 bytes, nested
  repetition, and backreferences are rejected with an error string; mark 1 is
  every result and mark 2 identifies the current matching line.
- `kitty_session_search_next(s, backwards)` — move the kitty viewport to
  another matching line, wrapping at the ends.
- `kitty_session_search_refresh(s)` — recount after new output without
  resetting the viewport; `kitty_session_search_visible_mark_count` is a
  verification/introspection helper for the current visible grid.
- `kitty_session_search_clear(s)` — remove only this session's search
  marker/state; terminal text, history, selection, and mouse modes are
  untouched.
- `kitty_session_reported_cwd(s)` — return kitty's decoded
  `Screen.last_reported_cwd` as caller-freed UTF-8; empty until the child
  emits valid OSC 7.
- `kitty_session_encode_key(s, event, out, len)` — use kitty's keyboard
  encoder and the session's live protocol state to produce child input bytes.
- `kitty_session_mouse_event(s, cell_x, cell_y, button, action, mods, px, py)`
  — offer a mouse event to the terminal application (v0.7, additive). Writes
  the encoded event to the child and returns 1 iff the app enabled a
  tracking mode covering this action, in whichever protocol it selected;
  returns 0 with no side effects otherwise, so hosts can fall back to local
  selection/scrolling.
- `kitty_session_resize(s, lines, cols)` — resize grid and PTY (cell units).
- `kitty_session_text(s)` — screen contents as malloc'd UTF-8 (tests,
  debugging, accessibility; the non-GL path).
- `kitty_session_scrollback_text(s, max_lines, max_bytes)` — 03 scrollback
  snapshot API: return only the newest bounded physical rows from the live main
  screen plus kitty's in-memory history as caller-freed plain UTF-8. It reads
  non-visual rows directly, leaves viewport/selection/search untouched, and
  returns empty while an alternate screen is active.
- `kitty_session_replay_plain_text(s, data, len)` — 03 scrollback snapshot
  API: strip terminal controls, normalize lines for display, and feed the text
  directly into the kitty `Screen` without writing to the child PTY.
- `kitty_session_child_alive(s)`, `kitty_session_close(s)` — lifecycle.

For interactive zsh, bash, and fish sessions, v0.6 reuses kitty's own shell
integration in cwd-only mode so `kitty_session_reported_cwd` stays current.
`shell_integration disabled`, `no-rc`, and `no-cwd` are respected. Custom
shells must emit standard OSC 7 themselves; without OSC 7 the getter remains
empty and the terminal otherwise works normally.

**Rendering** (kitty's real shader pipeline; every call needs the target GL
context current):

- `kitty_render_init(e, scale, &cell_w, &cell_h, errbuf, len)` — once, with
  a GL context current: load GL, compile kitty's shaders, load fonts at the
  given backing scale (2.0 on Retina). Reports cell size in pixels. Create
  sessions *after* this so their cell geometry matches the fonts.
- `kitty_render_font_size(e)` — current shared renderer font size in points
  (`0` before render init).
- `kitty_render_set_font_size(e, points, &cell_w, &cell_h, errbuf, len)` —
  rebuild the shared kitty font data/atlas, update every live session's
  Screen cell metrics, and report the new backing-pixel cell size (v0.8,
  additive). The GL context must be current; existing sessions, terminal
  state, selection, and scrollback remain alive.
- `kitty_session_set_viewport(s, fb_w_px, fb_h_px)` — per view, after render
  init and on every view resize. Sizes the render target and resizes the
  grid + PTY to fill it. Required once before drawing.
- `kitty_session_set_geometry(s, fb_w, fb_h, left, top, right, bottom)` —
  place one session inside a sub-rectangle of a shared framebuffer. Each
  rendered session owns a separate cell VAO, allowing many panes in one
  native window.
- `kitty_session_release_render_resources(s)` — v0.21, additive: drop this
  session's *context-local* GL objects (its cell VAO) so the session can be
  drawn by a **different** GL context, as when a terminal surface moves
  between host windows. **The creating context must be current.** VAO names
  live in per-context namespaces while kitty's VAO registry is process-global,
  so releasing under the wrong context can delete an unrelated VAO and leaks
  this one — silently, with no GL error. Shader programs, the font atlas, and
  VAO buffers live in the context share group and are untouched, as is the
  session's render stub (a plain `OSWindow` struct with no GL objects). The
  session refuses to draw until `kitty_session_set_geometry` rebuilds it in
  whatever context is then current; that rebuild also forces a full GPU
  reload, without which the pane would draw its background and no glyphs.
  Proven by `tests/test_render_multi_context.c`.
- `kitty_session_draw(s)` — draw the screen into the currently bound
  framebuffer. The render stub honors parsed `background_opacity`; kitty's
  existing shader makes default-background cells translucent while keeping
  explicit/special cells opaque.
- `kitty_session_draw_with_state(s, cursor_visible, pane_active,
  window_focused, single_pane)` — additive v0.20 draw path that feeds the
  host's real pane and OS-window focus into Kitty's native
  `inactive_text_alpha` shader policy. The older draw functions remain
  source-compatible and behave as a focused single pane.

**Threading rules:** any thread may call any function; the GIL is handled
internally (verified under stress in Phase 2). Callbacks fire on the thread
that called `pump`; don't re-enter libkitty from a callback in v0.

## Minimal host

```c
char err[512];
kitty_engine_config cfg = {.kitty_src_path = "...", .libkitty_py_path = "..."};
kitty_engine *e = kitty_engine_init(&cfg, err, sizeof err);

// with your GL context current:
kitty_render_init(e, /*scale*/ 2.0, NULL, NULL, err, sizeof err);
kitty_session *s = kitty_session_create(e, 24, 80, NULL, &cbs, err, sizeof err);
kitty_session_set_viewport(s, fb_width_px, fb_height_px);

// per run-loop iteration (fd readable, or a timer):
kitty_session_pump(s);       // fires on_damage if the screen changed
// in your view's draw, GL context current:
kitty_session_draw(s);

kitty_session_close(s);
kitty_engine_shutdown(e);
```

The runnable version is [`examples/minimal_host.m`](examples/minimal_host.m):
~140 lines for a full AppKit window running your shell (`make run-example`).

## Runtime dependencies

- A built kitty checkout (`kitty_src_path`) — provides `kitty/` Python
  modules and the compiled `fast_data_types` extension.
- `py/glue.py` (`libkitty_py_path`).
- A CPython ≥3.14 runtime. Development and packaging use Kitty's pinned
  portable Python 3.14.6 toolchain through `../scripts/python-config.sh`.
  Packaged hosts set `python_home` to the embedded framework; libkitty then
  ignores ambient Python environment variables and disables bytecode writes
  so the signed bundle is not modified. See `../docs/packaging.md`.

## Current limitations

- Kitty graphics-protocol display is not verified yet. Scrollback moves in
  whole lines rather than using kitty's optional pixel-smooth path.
- `config_path` loads `kitty.conf` for engine options; per-session overrides
  and live config reload don't exist. Font-size changes are shared by the
  engine; per-session font sizes are not supported.
- One engine per process (CPython); sessions are cheap, create many.
- Child exit status is captured via blocking `waitpid` after PTY EOF; a child
  that closes its terminal but keeps running would stall a `pump` (rare;
  fixed when session IO moves off the pull model).
- `assert`-based tests require `-UNDEBUG` (the Makefile handles it —
  `python3-config` injects `-DNDEBUG`). Side-effecting calls therefore live
  outside `assert` in the test suite.
- Multi-context rendering is proven for the *correct* lifecycle only. The
  test suite cannot detect a host that releases render resources under the
  wrong current context, because a name collision is possible but not
  guaranteed and per-frame cell re-upload hides cross-session corruption. The
  contract on `kitty_session_release_render_resources` is a host discipline
  requirement, not something libkitty enforces.
