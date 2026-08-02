/*
 * Slice 2.2A deterministic keyboard matrix.
 *
 * Drives the GDK -> libkitty translation and kitty's own encoder over a fixed
 * table of key events, in three live terminal states, and asserts the exact
 * bytes each event produces. Every expectation below is written from kitty's
 * documented keyboard protocol and its pinned encoder (kitty/key_encoding.c),
 * not captured from this host's output.
 *
 * The bytes are then written to a real PTY child (tests/pty_input_recorder.c)
 * and compared against what the child actually read, so the proof covers the
 * whole path from GDK vocabulary to child input. No display is required; the
 * GTK widget/focus path is proven separately by scripts/test-desktop.sh.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <gdk/gdk.h>
#include <gdk/gdkkeysyms.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#include "gtk_key_translation.h"
#include "libkitty.h"

#define CHECK(condition, ...)                                \
  do {                                                       \
    if (!(condition)) {                                      \
      fprintf(stderr, "FAIL %s:%d: ", __FILE__, __LINE__);   \
      fprintf(stderr, __VA_ARGS__);                          \
      fprintf(stderr, "\n");                                 \
      failures++;                                            \
      return false;                                          \
    }                                                        \
  } while (0)

static int failures = 0;

// Terminal states the harness exercises. `init` is written by the child at
// startup, so the encoder reads it back out of kitty's live Screen.
typedef enum { MODE_LEGACY = 0, MODE_DECCKM = 1, MODE_ENHANCED = 2 } mode_index;

typedef struct {
  const char *name;
  const char *init;  // escape sequence the child emits at startup
} mode_spec;

static const mode_spec modes[] = {
    {"legacy", ""},
    {"decckm", "\\e[?1h"},      // DECSET 1: application cursor keys
    {"enhanced", "\\e[>15u"},   // kitty keyboard protocol, flags 1|2|4|8
};

typedef struct {
  const char *name;
  guint keyval;
  guint unshifted_keyval;
  GdkModifierType state;
  int action;
  // Translation expectations (the event metadata the host must produce).
  uint32_t key;
  uint32_t shifted_key;
  uint32_t mods;
  // What the host's GtkIMContext committed for this event, "" for nothing.
  // The translation never invents this; it is an input, not an expectation.
  const char *im_text;
  // Encoded bytes as hex, one column per mode. "" means "send nothing".
  const char *expected[3];
} key_case;

// clang-format off
static const key_case cases[] = {
  // Printable ASCII: press sends the text itself in legacy modes; the kitty
  // protocol reports every key as an escape code with an event type.
  {"a press",            GDK_KEY_a, GDK_KEY_a, 0, KITMUX_KEY_ACTION_PRESS,
   0x61, 0, 0, "a",   {"61", "61", "1b5b393775"}},
  {"a repeat",           GDK_KEY_a, GDK_KEY_a, 0, KITMUX_KEY_ACTION_REPEAT,
   0x61, 0, 0, "a",   {"61", "61", "1b5b39373b313a3275"}},
  {"a release",          GDK_KEY_a, GDK_KEY_a, 0, KITMUX_KEY_ACTION_RELEASE,
   0x61, 0, 0, "",    {"",   "",   "1b5b39373b313a3375"}},
  {"space press",        GDK_KEY_space, GDK_KEY_space, 0, KITMUX_KEY_ACTION_PRESS,
   0x20, 0, 0, " ",   {"20", "20", "1b5b333275"}},

  // Shift: the key value stays the base-layout codepoint and the shifted
  // codepoint travels alongside it.
  {"shift b press",      GDK_KEY_B, GDK_KEY_b, GDK_SHIFT_MASK, KITMUX_KEY_ACTION_PRESS,
   0x62, 0x42, KITMUX_KITTY_MOD_SHIFT, "B", {"42", "42", "1b5b39383a36363b3275"}},
  {"shift 2 press",      GDK_KEY_at, GDK_KEY_2, GDK_SHIFT_MASK, KITMUX_KEY_ACTION_PRESS,
   0x32, 0x40, KITMUX_KITTY_MOD_SHIFT, "@", {"40", "40", "1b5b35303a36343b3275"}},

  // Control/Alt/Super chords carry no text; the encoder synthesizes them.
  {"ctrl c press",       GDK_KEY_c, GDK_KEY_c, GDK_CONTROL_MASK, KITMUX_KEY_ACTION_PRESS,
   0x63, 0, KITMUX_KITTY_MOD_CTRL, "", {"03", "03", "1b5b39393b3575"}},
  {"ctrl shift c press", GDK_KEY_C, GDK_KEY_c, GDK_CONTROL_MASK | GDK_SHIFT_MASK,
   KITMUX_KEY_ACTION_PRESS,
   0x63, 0x43, KITMUX_KITTY_MOD_CTRL | KITMUX_KITTY_MOD_SHIFT, "",
   {"1b5b39393b3675", "1b5b39393b3675", "1b5b39393a36373b3675"}},
  {"ctrl space press",   GDK_KEY_space, GDK_KEY_space, GDK_CONTROL_MASK,
   KITMUX_KEY_ACTION_PRESS,
   0x20, 0, KITMUX_KITTY_MOD_CTRL, "", {"00", "00", "1b5b33323b3575"}},
  {"alt d press",        GDK_KEY_d, GDK_KEY_d, GDK_ALT_MASK, KITMUX_KEY_ACTION_PRESS,
   0x64, 0, KITMUX_KITTY_MOD_ALT, "", {"1b64", "1b64", "1b5b3130303b3375"}},
  {"super s press",      GDK_KEY_s, GDK_KEY_s, GDK_SUPER_MASK, KITMUX_KEY_ACTION_PRESS,
   0x73, 0, KITMUX_KITTY_MOD_SUPER, "",
   {"1b5b3131353b3975", "1b5b3131353b3975", "1b5b3131353b3975"}},

  // Enter / Tab / Backspace / Escape.
  {"enter press",        GDK_KEY_Return, GDK_KEY_Return, 0, KITMUX_KEY_ACTION_PRESS,
   0xE001, 0, 0, "", {"0d", "0d", "1b5b313375"}},
  {"enter repeat",       GDK_KEY_Return, GDK_KEY_Return, 0, KITMUX_KEY_ACTION_REPEAT,
   0xE001, 0, 0, "", {"0d", "0d", "1b5b31333b313a3275"}},
  {"enter release",      GDK_KEY_Return, GDK_KEY_Return, 0, KITMUX_KEY_ACTION_RELEASE,
   0xE001, 0, 0, "", {"", "", "1b5b31333b313a3375"}},
  {"tab press",          GDK_KEY_Tab, GDK_KEY_Tab, 0, KITMUX_KEY_ACTION_PRESS,
   0xE002, 0, 0, "", {"09", "09", "1b5b3975"}},
  {"shift tab press",    GDK_KEY_ISO_Left_Tab, GDK_KEY_Tab, GDK_SHIFT_MASK,
   KITMUX_KEY_ACTION_PRESS,
   0xE002, 0, KITMUX_KITTY_MOD_SHIFT, "", {"1b5b5a", "1b5b5a", "1b5b393b3275"}},
  {"backspace press",    GDK_KEY_BackSpace, GDK_KEY_BackSpace, 0, KITMUX_KEY_ACTION_PRESS,
   0xE003, 0, 0, "", {"7f", "7f", "1b5b31323775"}},
  {"ctrl backspace press", GDK_KEY_BackSpace, GDK_KEY_BackSpace, GDK_CONTROL_MASK,
   KITMUX_KEY_ACTION_PRESS,
   0xE003, 0, KITMUX_KITTY_MOD_CTRL, "", {"08", "08", "1b5b3132373b3575"}},
  {"escape press",       GDK_KEY_Escape, GDK_KEY_Escape, 0, KITMUX_KEY_ACTION_PRESS,
   0xE000, 0, 0, "", {"1b", "1b", "1b5b323775"}},

  // Arrows: DECCKM is live Screen state, so the same event encodes
  // differently once the child turns application cursor keys on.
  {"up press",           GDK_KEY_Up, GDK_KEY_Up, 0, KITMUX_KEY_ACTION_PRESS,
   0xE008, 0, 0, "", {"1b5b41", "1b4f41", "1b5b41"}},
  {"up release",         GDK_KEY_Up, GDK_KEY_Up, 0, KITMUX_KEY_ACTION_RELEASE,
   0xE008, 0, 0, "", {"", "", "1b5b313b313a3341"}},
  {"down press",         GDK_KEY_Down, GDK_KEY_Down, 0, KITMUX_KEY_ACTION_PRESS,
   0xE009, 0, 0, "", {"1b5b42", "1b4f42", "1b5b42"}},
  {"left press",         GDK_KEY_Left, GDK_KEY_Left, 0, KITMUX_KEY_ACTION_PRESS,
   0xE006, 0, 0, "", {"1b5b44", "1b4f44", "1b5b44"}},
  {"right press",        GDK_KEY_Right, GDK_KEY_Right, 0, KITMUX_KEY_ACTION_PRESS,
   0xE007, 0, 0, "", {"1b5b43", "1b4f43", "1b5b43"}},
  {"shift up press",     GDK_KEY_Up, GDK_KEY_Up, GDK_SHIFT_MASK, KITMUX_KEY_ACTION_PRESS,
   0xE008, 0, KITMUX_KITTY_MOD_SHIFT, "",
   {"1b5b313b3241", "1b5b313b3241", "1b5b313b3241"}},
  {"home press",         GDK_KEY_Home, GDK_KEY_Home, 0, KITMUX_KEY_ACTION_PRESS,
   0xE00C, 0, 0, "", {"1b5b48", "1b4f48", "1b5b48"}},
  {"page up press",      GDK_KEY_Page_Up, GDK_KEY_Page_Up, 0, KITMUX_KEY_ACTION_PRESS,
   0xE00A, 0, 0, "", {"1b5b357e", "1b5b357e", "1b5b357e"}},

  // Non-US layouts and AltGr levels reach the encoder as an ordinary key
  // whose text the input method supplied. The key identity stays the
  // unmodified symbol of the same hardware key in the active layout, so a
  // German AltGr+q that produces "@" still reports q.
  {"de udiaeresis press", GDK_KEY_udiaeresis, GDK_KEY_udiaeresis, 0,
   KITMUX_KEY_ACTION_PRESS,
   0xFC, 0, 0, "\xc3\xbc", {"c3bc", "c3bc", "1b5b32353275"}},
  {"de altgr q press",   GDK_KEY_at, GDK_KEY_q, 0, KITMUX_KEY_ACTION_PRESS,
   0x71, 0, 0, "@", {"40", "40", "1b5b31313375"}},

  // Function keys: F1-F4 keep their legacy SS3 forms, F5+ are CSI ~ forms.
  {"f1 press",           GDK_KEY_F1, GDK_KEY_F1, 0, KITMUX_KEY_ACTION_PRESS,
   0xE014, 0, 0, "", {"1b4f50", "1b4f50", "1b5b50"}},
  {"f5 press",           GDK_KEY_F5, GDK_KEY_F5, 0, KITMUX_KEY_ACTION_PRESS,
   0xE018, 0, 0, "", {"1b5b31357e", "1b5b31357e", "1b5b31357e"}},
  {"f12 press",          GDK_KEY_F12, GDK_KEY_F12, 0, KITMUX_KEY_ACTION_PRESS,
   0xE01F, 0, 0, "", {"1b5b32347e", "1b5b32347e", "1b5b32347e"}},
};
// clang-format on

static void append_hex(char *out, size_t capacity, const char *bytes,
                       size_t length) {
  size_t used = strlen(out);
  for (size_t i = 0; i < length && used + 3 < capacity; ++i) {
    snprintf(out + used, capacity - used, "%02x", (unsigned char)bytes[i]);
    used += 2;
  }
}

static bool wait_readable(int fd, int timeout_ms) {
  struct pollfd descriptor = {.fd = fd, .events = POLLIN};
  int ready = poll(&descriptor, 1, timeout_ms);
  return ready > 0;
}

// Concatenate every complete "bytes <hex>" line the fixture has flushed.
static bool read_recorder_hex(const char *path, char *out, size_t capacity,
                              bool *ready) {
  out[0] = '\0';
  *ready = false;
  FILE *log = fopen(path, "r");
  if (!log) return false;
  char line[8192];
  size_t used = 0;
  while (fgets(line, sizeof(line), log)) {
    size_t length = strlen(line);
    if (length == 0 || line[length - 1] != '\n') break;  // partial flush
    line[length - 1] = '\0';
    if (strcmp(line, "ready") == 0) {
      *ready = true;
      continue;
    }
    if (strncmp(line, "bytes ", 6) != 0) continue;
    size_t payload = strlen(line + 6);
    if (used + payload + 1 >= capacity) break;
    memcpy(out + used, line + 6, payload);
    used += payload;
    out[used] = '\0';
  }
  fclose(log);
  return true;
}

static bool run_mode(kitty_engine *engine, const char *recorder,
                     const char *log_directory, mode_index mode) {
  const mode_spec *spec = &modes[mode];
  char log_path[1024];
  snprintf(log_path, sizeof(log_path), "%s/recorder-%s.log", log_directory,
           spec->name);
  unlink(log_path);

  char log_variable[1100];
  char init_variable[256];
  snprintf(log_variable, sizeof(log_variable), "KITMUX_RECORDER_LOG=%s",
           log_path);
  snprintf(init_variable, sizeof(init_variable), "KITMUX_RECORDER_INIT=%s",
           spec->init);
  const char *const argv[] = {recorder, NULL};
  const char *const child_environment[] = {log_variable, init_variable, NULL};

  char error[1024] = {0};
  kitty_session *session = kitty_session_create_with_options(
      engine, 24, 80, argv, NULL, child_environment, NULL, error,
      sizeof(error));
  CHECK(session != NULL, "%s: session creation failed: %s", spec->name, error);

  // The fixture's startup write is the only output at this point, so the
  // first pump that reads bytes is the proof its mode reached kitty's Screen.
  int fd = kitty_session_fd(session);
  bool applied = false;
  for (int attempt = 0; attempt < 100 && !applied; ++attempt) {
    if (!wait_readable(fd, 100)) continue;
    kitty_session_pump(session);
    applied = kitty_session_last_pump_bytes(session) > 0;
  }
  CHECK(applied, "%s: child never produced its startup output", spec->name);

  char expected_stream[65536] = {0};
  size_t expected_length = 0;
  for (size_t i = 0; i < G_N_ELEMENTS(cases); ++i) {
    const key_case *item = &cases[i];
    kitmux_gdk_key_input input = {
        .keyval = item->keyval,
        .unshifted_keyval = item->unshifted_keyval,
        .state = item->state,
        .action = item->action,
    };
    kitmux_key_translation translated;
    CHECK(kitmux_translate_gdk_key(&input, item->im_text, &translated),
          "%s/%s: translation rejected an encodable key", spec->name,
          item->name);
    CHECK(translated.event.key == item->key,
          "%s/%s: key 0x%X, expected 0x%X", spec->name, item->name,
          translated.event.key, item->key);
    CHECK(translated.event.shifted_key == item->shifted_key,
          "%s/%s: shifted_key 0x%X, expected 0x%X", spec->name, item->name,
          translated.event.shifted_key, item->shifted_key);
    CHECK(translated.event.mods == item->mods,
          "%s/%s: mods 0x%X, expected 0x%X", spec->name, item->name,
          translated.event.mods, item->mods);
    CHECK(strcmp(translated.event.text, item->im_text) == 0,
          "%s/%s: committed text \"%s\" did not survive translation as \"%s\"",
          spec->name, item->name, translated.event.text, item->im_text);

    char encoded[256];
    size_t written =
        kitty_session_encode_key(session, &translated.event, encoded,
                                 sizeof(encoded));
    char hex[520] = {0};
    append_hex(hex, sizeof(hex), encoded, written);
    CHECK(strcmp(hex, item->expected[mode]) == 0,
          "%s/%s: encoded %s, expected %s", spec->name, item->name, hex,
          item->expected[mode]);

    if (written > 0) {
      kitty_session_write(session, (const uint8_t *)encoded, written);
      CHECK(expected_length + strlen(hex) + 1 < sizeof(expected_stream),
            "%s: expectation buffer overflow", spec->name);
      memcpy(expected_stream + expected_length, hex, strlen(hex));
      expected_length += strlen(hex);
      expected_stream[expected_length] = '\0';
    }
  }

  // The fixture is a real child on a real PTY: wait for it to read everything.
  char recorded[65536] = {0};
  bool ready = false;
  bool complete = false;
  for (int attempt = 0; attempt < 200 && !complete; ++attempt) {
    if (!read_recorder_hex(log_path, recorded, sizeof(recorded), &ready)) {
      struct timespec pause = {.tv_sec = 0, .tv_nsec = 25 * 1000 * 1000};
      nanosleep(&pause, NULL);
      continue;
    }
    complete = strlen(recorded) >= expected_length;
    if (!complete) {
      struct timespec pause = {.tv_sec = 0, .tv_nsec = 25 * 1000 * 1000};
      nanosleep(&pause, NULL);
    }
  }
  CHECK(ready, "%s: fixture never reported readiness", spec->name);
  CHECK(strcmp(recorded, expected_stream) == 0,
        "%s: child read\n  %s\nexpected\n  %s", spec->name, recorded,
        expected_stream);

  kitty_session_close(session);
  printf("key matrix %s: %zu events, %zu bytes delivered to the child\n",
         spec->name, G_N_ELEMENTS(cases), expected_length / 2);
  return true;
}

static bool run_tracker_checks(void) {
  kitmux_key_tracker tracker = {0};
  CHECK(kitmux_key_tracker_press(&tracker, 38) == KITMUX_KEY_ACTION_PRESS,
        "first press must report a press");
  CHECK(kitmux_key_tracker_press(&tracker, 38) == KITMUX_KEY_ACTION_REPEAT,
        "a held key must report a repeat");
  CHECK(kitmux_key_tracker_press(&tracker, 38) == KITMUX_KEY_ACTION_REPEAT,
        "further presses of a held key must stay repeats");
  CHECK(kitmux_key_tracker_press(&tracker, 39) == KITMUX_KEY_ACTION_PRESS,
        "a second key must report its own press");
  CHECK(kitmux_key_tracker_held(&tracker) == 2, "two keys must be held");
  CHECK(kitmux_key_tracker_release(&tracker, 38),
        "releasing a held key must report that it was held");
  CHECK(!kitmux_key_tracker_release(&tracker, 38),
        "releasing an untracked key must report that it was not held");
  kitmux_key_tracker_press(&tracker, 38);
  kitmux_key_tracker_release(&tracker, 38);
  CHECK(kitmux_key_tracker_press(&tracker, 38) == KITMUX_KEY_ACTION_PRESS,
        "a released key must report a fresh press");
  kitmux_key_tracker_reset(&tracker);
  CHECK(kitmux_key_tracker_held(&tracker) == 0,
        "focus loss must release every held key");
  CHECK(kitmux_key_tracker_press(&tracker, 39) == KITMUX_KEY_ACTION_PRESS,
        "a reset tracker must report a fresh press");

  // Bare modifiers carry neither a functional value nor a codepoint.
  kitmux_gdk_key_input modifier = {.keyval = GDK_KEY_Shift_L,
                                   .unshifted_keyval = GDK_KEY_Shift_L,
                                   .state = 0,
                                   .action = KITMUX_KEY_ACTION_PRESS};
  kitmux_key_translation translated;
  CHECK(!kitmux_translate_gdk_key(&modifier, NULL, &translated),
        "a bare modifier must not reach the terminal");
  printf("key tracker: press/repeat/release and modifier rejection OK\n");
  return true;
}

int main(void) {
  const char *recorder = getenv("KITMUX_RECORDER_BIN");
  const char *log_directory = getenv("KITMUX_RECORDER_LOG_DIR");
  if (!recorder || !log_directory) {
    fprintf(stderr,
            "gtk_key_matrix: KITMUX_RECORDER_BIN and KITMUX_RECORDER_LOG_DIR "
            "are required\n");
    return 2;
  }

  kitty_engine_config config = {
      .kitty_src_path = getenv("KITTY_SRC"),
      .libkitty_py_path = getenv("LIBKITTY_PY"),
      .python_home = getenv("PYTHONHOME"),
      .config_path = getenv("LIBKITTY_TEST_CONFIG"),
  };
  char error[1024] = {0};
  kitty_engine *engine = kitty_engine_init(&config, error, sizeof(error));
  if (!engine) {
    fprintf(stderr, "gtk_key_matrix: engine init failed: %s\n", error);
    return 2;
  }

  // Every CHECK already records its own failure; these calls only stop the
  // current block early.
  run_tracker_checks();
  for (size_t mode = 0; mode < G_N_ELEMENTS(modes); ++mode) {
    run_mode(engine, recorder, log_directory, (mode_index)mode);
  }
  kitty_engine_shutdown(engine);

  if (failures > 0) {
    fprintf(stderr, "gtk_key_matrix: %d failing check(s)\n", failures);
    return 1;
  }
  printf("gtk_key_matrix: all keyboard expectations passed\n");
  return 0;
}
