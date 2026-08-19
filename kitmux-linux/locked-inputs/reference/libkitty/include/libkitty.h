/*
 * libkitty.h — v0
 *
 * Embeddable terminal engine extracted from kitty. One engine per process
 * (it owns an embedded CPython interpreter); any number of sessions, each
 * a kitty Screen wired to a child process on a PTY.
 *
 * Threading: any thread may call any function; the GIL is handled
 * internally. Callbacks fire on the thread that called kitty_session_pump.
 * GL functions (kitty_render_init, kitty_session_set_viewport,
 * kitty_session_set_geometry, kitty_session_draw) must be called with the
 * target GL context current.
 *
 * License: GPLv3, same as kitty.
 */
#ifndef LIBKITTY_H
#define LIBKITTY_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct kitty_engine kitty_engine;    // opaque: owns the embedded interpreter
typedef struct kitty_session kitty_session;  // opaque: one Screen + one child process

typedef struct kitty_engine_config {
    const char *kitty_src_path;    // kitty checkout with built fast_data_types (import root)
    const char *libkitty_py_path;  // directory containing libkitty's glue.py
    const char *python_home;       // bundled Python runtime prefix; NULL = environment default
    const char *config_path;       // kitty.conf to load; NULL = kitty defaults
} kitty_engine_config;

typedef struct kitty_session_callbacks {
    void *userdata;
    void (*on_damage)(void *userdata);                        // screen changed; schedule a redraw
    void (*on_title)(void *userdata, const char *title);      // child set the window title
    void (*on_bell)(void *userdata);
    void (*on_child_exit)(void *userdata, int exit_status);   // fires once, from pump
    // v0.9 additive: fires once per OSC 9/99/777 desktop notification parsed
    // from child output, in arrival order, from kitty_session_pump. title and
    // body are UTF-8 with control characters stripped; body may be empty;
    // both are borrowed for the duration of the call. NULL to ignore.
    void (*on_notification)(void *userdata, const char *title, const char *body);
    // v0.11 additive: fires once per OSC 1337 SetUserVar captured from child
    // output, in arrival order, from kitty_session_pump. key and value are
    // UTF-8, borrowed for the call; an empty value means the variable was
    // cleared. NULL to ignore. libkitty forwards every user var; the host
    // decides which keys it cares about and applies its own value policy.
    void (*on_user_var)(void *userdata, const char *key, const char *value);
} kitty_session_callbacks;

// Engine lifecycle. Init once per process. On failure returns NULL and
// writes a message into errbuf (if non-NULL).
kitty_engine *kitty_engine_init(const kitty_engine_config *cfg, char *errbuf, size_t errbuf_len);
void kitty_engine_shutdown(kitty_engine *e);

// Session lifecycle. argv is NULL-terminated; NULL means the user's login shell.
// Callbacks may be NULL (or have NULL members) to opt out.
kitty_session *kitty_session_create(kitty_engine *e, int lines, int cols,
                                    const char *const *argv,
                                    const kitty_session_callbacks *cbs,
                                    char *errbuf, size_t errbuf_len);
// v0.6 additive creation path. Starts the same fresh child/session as
// kitty_session_create, but changes the child to cwd before exec when cwd is
// an existing accessible directory. Invalid or missing cwd falls back to the
// normal launch directory.
kitty_session *kitty_session_create_with_cwd(kitty_engine *e, int lines, int cols,
                                             const char *const *argv, const char *cwd,
                                             const kitty_session_callbacks *cbs,
                                             char *errbuf, size_t errbuf_len);
// v0.19 additive creation path. `env` is a NULL-terminated list of KEY=VALUE
// overrides applied after kitty.conf's env directives. It lets an embedding
// host give each child stable local identity without mutating the host process
// environment. NULL preserves the existing behavior.
kitty_session *kitty_session_create_with_options(kitty_engine *e, int lines, int cols,
                                                 const char *const *argv, const char *cwd,
                                                 const char *const *env,
                                                 const kitty_session_callbacks *cbs,
                                                 char *errbuf, size_t errbuf_len);
void kitty_session_close(kitty_session *s);   // SIGHUP + reap child, free everything
bool kitty_session_child_alive(kitty_session *s);
int  kitty_session_child_pid(kitty_session *s);  // direct child PID, stable until close
// v0.17: true when the PTY has a live foreground process group distinct from
// its child shell. A previously-foreground stopped job also remains true;
// running background jobs do not.
bool kitty_session_has_foreground_process(kitty_session *s);
int  kitty_session_fd(kitty_session *s);      // master PTY fd, for host run-loop integration
bool kitty_session_pump(kitty_session *s);    // parse pending child output, flush replies,
                                              // fire callbacks; true if the screen changed
// Verification/observability aid: bytes read by the most recent pump turn.
// Zero means the turn read no child output. This does not perform another read.
size_t kitty_session_last_pump_bytes(kitty_session *s);
void kitty_session_write(kitty_session *s, const uint8_t *data, size_t len);  // bytes -> child
void kitty_session_resize(kitty_session *s, int lines, int cols);             // grid + PTY size

// Paste with kitty's semantics (v0.3, additive): honors the terminal's
// bracketed-paste mode (2004) — wraps with ESC[200~/201~ and strips embedded
// end markers when set; normalizes newlines to \r when not. Use this for
// clipboard pastes; kitty_session_write for raw keystroke bytes.
void kitty_session_paste(kitty_session *s, const uint8_t *data, size_t len);

// Scrollback (v0.4, additive). The viewport offset lives in kitty's Screen,
// so per-session isolation and anchoring are kitty's own behavior: while
// scrolled up, new output keeps the view anchored on the same content, and
// a key press snaps back to the bottom (kitty parity).
//
// Scroll the viewport by `lines` (+ = up into history, - = down; clamped to
// the history size). On the alternate screen (vim, less) this mirrors
// kitty's wheel behavior instead: encoded Up/Down arrow keys to the child.
void kitty_session_scroll(kitty_session *s, int lines);
// Drop the history buffer and reset the viewport (kitty's
// "clear_terminal scrollback"). The live screen is untouched.
void kitty_session_clear_scrollback(kitty_session *s);
// Current viewport offset in lines; 0 = at the live bottom.
unsigned int kitty_session_scrolled_by(kitty_session *s);
// Number of currently available main-screen history rows; zero on alternate screen.
unsigned int kitty_session_history_line_count(kitty_session *s);

// F7: current cursor cell (0-based column/row in the visible grid). visible
// is false while scrolled back or the program hid the cursor (DECTCEM).
// Returns false on error.
bool kitty_session_cursor_cell(kitty_session *s, int *x, int *y, bool *visible);

// Text selection (v0.5, additive). Coordinates are zero-based cells in the
// currently visible viewport. Selection state lives in kitty's Screen, so it
// is isolated per session and remains tied to history while scrolled back.
// extend_mode (v0.12): 0 = cell (drag), 1 = word (double-click), 2 = line
// (triple-click). kitty's engine owns the word/line boundary logic; dragging
// after a word/line start extends by whole words/lines.
void kitty_session_selection_start(kitty_session *s, unsigned int column,
                                   unsigned int row, bool in_left_half_of_cell,
                                   unsigned int extend_mode);
void kitty_session_selection_update(kitty_session *s, unsigned int column,
                                    unsigned int row, bool in_left_half_of_cell,
                                    bool ended);
void kitty_session_selection_clear(kitty_session *s);
// Plain UTF-8 selected text. malloc'd; caller frees. Returns an empty string
// when the session has no non-empty selection.
char *kitty_session_selection_text(kitty_session *s);

// Search state and native mark highlighting live in this session's kitty
// Screen and include both the live main screen and scrollback. The original
// v0.10 entry point remains literal/case-insensitive. The additive options
// entry point supports case-sensitive and bounded regex modes; false means an
// invalid/unsafe regex and writes a user-facing message to errbuf. Mark 1 is
// every match and mark 2 is the current line-level navigation result. refresh
// updates the count after new output. clear leaves selection/text/history.
size_t kitty_session_search_set(kitty_session *s,
                                const char *query, size_t query_len);
bool kitty_session_search_set_options(kitty_session *s,
                                      const char *query, size_t query_len,
                                      bool case_sensitive, bool regex,
                                      size_t *match_count,
                                      char *errbuf, size_t errbuf_len);
bool kitty_session_search_next(kitty_session *s, bool backwards);
size_t kitty_session_search_refresh(kitty_session *s);
size_t kitty_session_search_visible_mark_count(kitty_session *s,
                                               unsigned int mark);
void kitty_session_search_clear(kitty_session *s);

// Most recent local path reported by the child via OSC 7, decoded using
// kitty's own URL rules. malloc'd; caller frees. Empty when no valid OSC 7
// has been received.
char *kitty_session_reported_cwd(kitty_session *s);

// Screen contents as UTF-8, one line per '\n'. malloc'd; caller frees.
// The non-GL content path (tests, accessibility, debugging).
char *kitty_session_text(kitty_session *s);

// One byte per visible row: nonzero when that row soft-wraps into the next.
// Returns the number of bytes written, bounded by capacity. This preserves
// kitty's exact continuation metadata for host-side URL hit-testing.
size_t kitty_session_line_wraps(kitty_session *s, uint8_t *out,
                                size_t capacity);

// 03 scrollback snapshot API (v0.14, additive).
// Return the newest bounded plain-text main screen + in-memory history
// snapshot without changing viewport, selection, or search state. max_lines
// and max_bytes are hard output caps. Returns an allocated empty string for
// zero caps, empty content, or an active alternate screen. Caller frees.
char *kitty_session_scrollback_text(kitty_session *s,
                                    size_t max_lines, size_t max_bytes);

// Display sanitized plain UTF-8 in the Screen as terminal output without
// writing any bytes to the child PTY. C0/DEL controls are stripped except tab
// and newline; line endings become CRLF.
bool kitty_session_replay_plain_text(kitty_session *s,
                                     const uint8_t *data, size_t len);

// Rendering. Call once with a GL context current, before any draw.
// scale is the backing scale factor (2.0 on Retina). Reports cell size in
// pixels via out params (either may be NULL).
bool kitty_render_init(kitty_engine *e, double scale,
                       int *cell_width_px, int *cell_height_px,
                       char *errbuf, size_t errbuf_len);
// v0.8 additive font controls. The getter returns 0 until render init.
// Setting requires the host GL context to be current, updates all live
// sessions' Screen cell metrics, and reports the new backing-pixel cell size.
double kitty_render_font_size(kitty_engine *e);
bool kitty_render_set_font_size(kitty_engine *e, double font_size_points,
                                int *cell_width_px, int *cell_height_px,
                                char *errbuf, size_t errbuf_len);

// Per-view sizing: call after render init and again on every view resize,
// GL context current. Sizes the render viewport AND resizes the terminal
// grid + PTY to fill it. Must be called once before kitty_session_draw.
bool kitty_session_set_viewport(kitty_session *s, int fb_width_px, int fb_height_px);

// v0.21 additive: release this session's context-local GL objects (its VAO) so
// the session can be drawn by a DIFFERENT GL context. Required when a terminal
// surface moves between host windows.
//
// MUST be called with the GL context that created those resources current.
// glGenVertexArrays names are per-context while kitty's VAO registry is
// process-global, so calling this under the wrong current context deletes an
// unrelated VAO in that context and leaks this one. The violation cannot be
// detected here and does not raise a GL error.
//
// Shader programs, the font atlas, and VAO buffers live in the context share
// group and are untouched. The session keeps running; only its GPU-side draw
// state is dropped.
//
// After this call the session refuses to draw until kitty_session_set_geometry
// runs again, which rebuilds the VAO in whatever context is then current.
bool kitty_session_release_render_resources(kitty_session *s);

// Draw the session's screen into the currently bound framebuffer.
bool kitty_session_draw(kitty_session *s);
// Backlog 05 additive draw path: identical to kitty_session_draw, except a
// false host cursor phase temporarily suppresses the terminal cursor for this
// frame without changing the child's DECTCEM state. The original entry point
// remains source-compatible and always draws the cursor normally.
bool kitty_session_draw_with_cursor_visibility(kitty_session *s,
                                               bool cursor_visible);
// Additive host-state draw path. pane_active identifies the focused pane in
// the visible split, window_focused is the host OS window's key state, and
// single_pane reports whether the tab has only one visible pane. These values
// feed kitty's native inactive_text_alpha policy, including its distinct
// positive/negative semantics. Older draw entry points retain their original
// fully-active behavior.
bool kitty_session_draw_with_state(kitty_session *s,
                                   bool cursor_visible,
                                   bool pane_active,
                                   bool window_focused,
                                   bool single_pane);

// ---- v0.1 additions (Phase C): keyboard encoding ----

typedef struct kitty_key_event {
    uint32_t key;            // unicode codepoint, or a GLFW_FKEY_* functional
                             // value (kitty numbering: ESCAPE=0xE000,
                             // ENTER=0xE001, TAB=0xE002, BACKSPACE=0xE003,
                             // INSERT=0xE004, DELETE=0xE005, LEFT=0xE006,
                             // RIGHT=0xE007, UP=0xE008, DOWN=0xE009,
                             // PAGE_UP=0xE00A, PAGE_DOWN=0xE00B, HOME=0xE00C,
                             // END=0xE00D, F1=0xE014 .. F12=0xE01F)
    uint32_t shifted_key;    // shifted variant codepoint, or 0
    uint32_t alternate_key;  // base-layout variant codepoint, or 0
    uint32_t mods;           // kitty mods: shift=0x1, alt=0x2, ctrl=0x4, super=0x8
    int action;              // 0=release, 1=press, 2=repeat
    const char *text;        // composed text for the event, or NULL
} kitty_key_event;

// Encode a key event using kitty's encoder AND the session's live terminal
// state (DECCKM cursor-key mode, kitty keyboard-protocol flag stack).
// Returns the number of bytes written into out; 0 means nothing should be
// sent for this event (e.g. a release when the protocol doesn't want them).
size_t kitty_session_encode_key(kitty_session *s, const kitty_key_event *ev,
                                char *out, size_t out_len);

// kitty's macos_option_as_alt option: 0=no, 0b10=left, 0b01=right, 0b11=both.
int kitty_engine_option_as_alt(kitty_engine *e);

// v0.13: read one numeric value from the parsed kitty configuration. Color
// values are returned as 0xRRGGBB via their `rgb` property. Returns false for
// an unknown/non-numeric key or invalid arguments; true and writes `out`
// otherwise. This keeps config application in the embedding host while
// reusing kitty's real parser.
bool kitty_engine_config_number(kitty_engine *e, const char *key, double *out);

// v0.16 additive: read one UTF-8 string value from parsed kitty
// configuration. The result is malloc'd; caller frees. Returns NULL for an
// unknown/non-string key or invalid arguments.
char *kitty_engine_config_string(kitty_engine *e, const char *key);

// v0.15: re-read a kitty.conf and re-apply engine-consumed options. Colors
// (fore/background, palette, selection/mark/cursor colors) re-apply to every
// live session's Screen through kitty's own ColorProfile reload; the screens
// are marked dirty so the next draw shows them. Font data and per-session
// creation-time values are NOT touched — the host decides how to badge those.
// Returns the number of live sessions patched (>= 0). On any failure returns
// -1, writes a message into errbuf, and leaves the previously loaded
// configuration fully active. config_path NULL reloads kitty defaults.
int kitty_engine_reload_config(kitty_engine *e, const char *config_path,
                               char *errbuf, size_t errbuf_len);

// ---- v0.2 additions (Phase B): multi-session compositing ----

// Place a session's cells within a larger framebuffer (kitty's own model:
// many windows composited into one OS window). viewport = the whole
// framebuffer; (left,top,right,bottom) = this session's region in pixels,
// top-left origin. Resizes the grid + PTY to fit the region. Call with the
// GL context current, after kitty_render_init, and again whenever layout
// changes. kitty_session_set_viewport(s,w,h) == set_geometry(s,w,h,0,0,w,h).
// Note: close rendered sessions with their GL context current (per-session
// GPU objects are released at close).
bool kitty_session_set_geometry(kitty_session *s, int fb_width_px, int fb_height_px,
                                int left, int top, int right, int bottom);

// ---- v0.7 additions: terminal-application mouse reporting ----

// Mouse actions for kitty_session_mouse_event.
#define KITTY_MOUSE_PRESS   0
#define KITTY_MOUSE_RELEASE 1
#define KITTY_MOUSE_DRAG    2
#define KITTY_MOUSE_MOVE    3

// Offer a mouse event to the terminal application. Encodes and writes it to
// the child iff the application enabled a tracking mode (DECSET 9/1000/1002/
// 1003) that covers this action, using whichever protocol it selected
// (legacy, UTF-8, URXVT, SGR, SGR-pixel); returns 1 then, 0 otherwise with
// no side effects. Buttons use kitty numbering: 1 left, 2 middle, 3 right,
// 4/5 wheel up/down, 6/7 wheel left/right. mods uses the same bits as
// kitty_key_event (shift=0x1, alt=0x2, ctrl=0x4). cell_* are zero-based
// viewport cells; pixel_* are offsets into the session's region in device
// pixels (used only by the SGR-pixel protocol, DECSET 1016).
int kitty_session_mouse_event(kitty_session *s,
                              unsigned int cell_x, unsigned int cell_y,
                              int button, int action, uint32_t mods,
                              int pixel_x, int pixel_y);

#endif
