/*
 * GDK key vocabulary -> libkitty key contract.
 *
 * Slice 2.2A owns only the physical key path: functional-key numbering,
 * kitty's GLFW-fork modifier bits, press/release/repeat actions, and the
 * text-suppression rules kitty's own backends apply. Encoding itself stays
 * inside kitty (kitty_session_encode_key), which also supplies the live
 * DECCKM and keyboard-protocol state.
 *
 * Nothing here touches a GdkDisplay, so the translation is testable without
 * a display server. The host resolves the level-0 keyval for the pressed
 * hardware key and passes it in.
 *
 * Slice 2.2B replaces the `text` synthesis below with a real GtkIMContext;
 * until then this file must not be treated as Compose/dead-key/IME evidence.
 */
#ifndef KITMUX_GTK_KEY_TRANSLATION_H
#define KITMUX_GTK_KEY_TRANSLATION_H

#include <gdk/gdk.h>
#include <stdbool.h>
#include <stddef.h>

#include "libkitty.h"

// kitty's GLFW fork: ALT=0x2, CONTROL=0x4 (not the X11 ordering).
#define KITMUX_KITTY_MOD_SHIFT 0x1u
#define KITMUX_KITTY_MOD_ALT 0x2u
#define KITMUX_KITTY_MOD_CTRL 0x4u
#define KITMUX_KITTY_MOD_SUPER 0x8u

// kitty action numbering, as documented on kitty_key_event.
#define KITMUX_KEY_ACTION_RELEASE 0
#define KITMUX_KEY_ACTION_PRESS 1
#define KITMUX_KEY_ACTION_REPEAT 2

// One UTF-8 scalar plus a terminator is all a single key event can carry.
#define KITMUX_KEY_TEXT_CAPACITY 8

typedef struct {
  guint keyval;            // keyval GDK delivered for this event
  guint unshifted_keyval;  // level-0 keyval of the same hardware key, or 0
  GdkModifierType state;   // modifier state at event time
  int action;              // KITMUX_KEY_ACTION_*
} kitmux_gdk_key_input;

typedef struct {
  kitty_key_event event;  // event.text points into this struct's text storage
  char text[KITMUX_KEY_TEXT_CAPACITY];
} kitmux_key_translation;

// Translate one GDK key event. Returns false when the event carries nothing
// the terminal should see (a bare modifier, or a keyval with no functional
// mapping and no Unicode value); `out` is then left zeroed.
bool kitmux_translate_gdk_key(const kitmux_gdk_key_input *input,
                              kitmux_key_translation *out);

// GDK reports auto-repeat as further key-pressed events without an
// intervening release (GDK enables X11 detectable auto-repeat), so the host
// keeps its own held-key set to tell press from repeat.
#define KITMUX_KEY_TRACKER_CAPACITY 32

typedef struct {
  guint codes[KITMUX_KEY_TRACKER_CAPACITY];
  size_t count;
} kitmux_key_tracker;

// Returns KITMUX_KEY_ACTION_PRESS the first time a hardware key goes down and
// KITMUX_KEY_ACTION_REPEAT while it stays down. A full tracker degrades to
// reporting presses rather than dropping the key.
int kitmux_key_tracker_press(kitmux_key_tracker *tracker, guint keycode);
void kitmux_key_tracker_release(kitmux_key_tracker *tracker, guint keycode);
// Focus loss ends every key the widget can still observe.
void kitmux_key_tracker_reset(kitmux_key_tracker *tracker);
size_t kitmux_key_tracker_held(const kitmux_key_tracker *tracker);

#endif
