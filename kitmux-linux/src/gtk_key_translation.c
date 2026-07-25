#include "gtk_key_translation.h"

#include <gdk/gdkkeysyms.h>
#include <string.h>

typedef struct {
  guint keyval;
  uint32_t functional;
} functional_key;

// kitty's GLFW_FKEY_* numbering (glfw/glfw3.h in the pinned Kitty source);
// the same values the public libkitty header documents.
static const functional_key functional_keys[] = {
    {GDK_KEY_Escape, 0xE000},
    {GDK_KEY_Return, 0xE001},
    {GDK_KEY_KP_Enter, 0xE001},
    {GDK_KEY_ISO_Enter, 0xE001},
    {GDK_KEY_Tab, 0xE002},
    {GDK_KEY_ISO_Left_Tab, 0xE002},  // X11's own keyval for Shift+Tab
    {GDK_KEY_BackSpace, 0xE003},
    {GDK_KEY_Insert, 0xE004},
    {GDK_KEY_KP_Insert, 0xE004},
    {GDK_KEY_Delete, 0xE005},
    {GDK_KEY_KP_Delete, 0xE005},
    {GDK_KEY_Left, 0xE006},
    {GDK_KEY_KP_Left, 0xE006},
    {GDK_KEY_Right, 0xE007},
    {GDK_KEY_KP_Right, 0xE007},
    {GDK_KEY_Up, 0xE008},
    {GDK_KEY_KP_Up, 0xE008},
    {GDK_KEY_Down, 0xE009},
    {GDK_KEY_KP_Down, 0xE009},
    {GDK_KEY_Page_Up, 0xE00A},
    {GDK_KEY_KP_Page_Up, 0xE00A},
    {GDK_KEY_Page_Down, 0xE00B},
    {GDK_KEY_KP_Page_Down, 0xE00B},
    {GDK_KEY_Home, 0xE00C},
    {GDK_KEY_KP_Home, 0xE00C},
    {GDK_KEY_End, 0xE00D},
    {GDK_KEY_KP_End, 0xE00D},
    {GDK_KEY_F1, 0xE014},
    {GDK_KEY_F2, 0xE015},
    {GDK_KEY_F3, 0xE016},
    {GDK_KEY_F4, 0xE017},
    {GDK_KEY_F5, 0xE018},
    {GDK_KEY_F6, 0xE019},
    {GDK_KEY_F7, 0xE01A},
    {GDK_KEY_F8, 0xE01B},
    {GDK_KEY_F9, 0xE01C},
    {GDK_KEY_F10, 0xE01D},
    {GDK_KEY_F11, 0xE01E},
    {GDK_KEY_F12, 0xE01F},
};

static bool functional_value(guint keyval, uint32_t *functional) {
  for (size_t i = 0; i < G_N_ELEMENTS(functional_keys); ++i) {
    if (functional_keys[i].keyval == keyval) {
      *functional = functional_keys[i].functional;
      return true;
    }
  }
  return false;
}

static uint32_t kitty_mods(GdkModifierType state) {
  uint32_t mods = 0;
  if (state & GDK_SHIFT_MASK) mods |= KITMUX_KITTY_MOD_SHIFT;
  if (state & GDK_CONTROL_MASK) mods |= KITMUX_KITTY_MOD_CTRL;
  if (state & GDK_ALT_MASK) mods |= KITMUX_KITTY_MOD_ALT;
  if (state & GDK_SUPER_MASK) mods |= KITMUX_KITTY_MOD_SUPER;
  return mods;
}

bool kitmux_translate_gdk_key(const kitmux_gdk_key_input *input,
                              const char *committed_text,
                              kitmux_key_translation *out) {
  if (!input || !out) return false;
  memset(out, 0, sizeof(*out));
  out->event.action = input->action;
  out->event.mods = kitty_mods(input->state);
  out->event.text = out->text;
  if (committed_text && *committed_text) {
    size_t length = strlen(committed_text);
    if (length < sizeof(out->text)) memcpy(out->text, committed_text, length);
  }

  uint32_t functional = 0;
  if (functional_value(input->keyval, &functional)) {
    out->event.key = functional;
    return true;
  }

  // Character key. The canonical key value is the base-layout codepoint,
  // lowercased, exactly as kitty's own GLFW backend reports it; the event's
  // own codepoint becomes shifted_key when Shift produced a different one.
  gunichar delivered = gdk_keyval_to_unicode(input->keyval);
  if (delivered == 0) return false;  // bare modifier, or a key with no text
  gunichar base = delivered;
  if (input->unshifted_keyval != 0) {
    gunichar unshifted = gdk_keyval_to_unicode(input->unshifted_keyval);
    if (unshifted != 0) base = unshifted;
  }
  gunichar key = g_unichar_tolower(base);
  out->event.key = (uint32_t)key;
  if ((out->event.mods & KITMUX_KITTY_MOD_SHIFT) && delivered != key) {
    out->event.shifted_key = (uint32_t)delivered;
  }
  return true;
}

int kitmux_key_tracker_press(kitmux_key_tracker *tracker, guint keycode) {
  if (!tracker) return KITMUX_KEY_ACTION_PRESS;
  for (size_t i = 0; i < tracker->count; ++i) {
    if (tracker->codes[i] == keycode) return KITMUX_KEY_ACTION_REPEAT;
  }
  if (tracker->count < KITMUX_KEY_TRACKER_CAPACITY) {
    tracker->codes[tracker->count++] = keycode;
  }
  return KITMUX_KEY_ACTION_PRESS;
}

bool kitmux_key_tracker_release(kitmux_key_tracker *tracker, guint keycode) {
  if (!tracker) return false;
  for (size_t i = 0; i < tracker->count; ++i) {
    if (tracker->codes[i] != keycode) continue;
    tracker->codes[i] = tracker->codes[tracker->count - 1];
    tracker->count--;
    return true;
  }
  return false;
}

void kitmux_key_tracker_reset(kitmux_key_tracker *tracker) {
  if (tracker) tracker->count = 0;
}

size_t kitmux_key_tracker_held(const kitmux_key_tracker *tracker) {
  return tracker ? tracker->count : 0;
}
