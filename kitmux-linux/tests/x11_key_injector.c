/*
 * Exact X11 key injection for the Slice 2.2A keyboard harness.
 *
 * xdotool is not usable here: it derives the modifiers to synthesize from the
 * shift level at which it finds a keysym, and this session's XKB map places
 * the function keys' XF86Switch_VT_* symbols on higher levels of the same
 * keycode. "xdotool key F1" therefore injects Alt+F1, which XFCE consumes as
 * a window-manager shortcut. A harness that cannot say exactly which keycodes
 * go down and up cannot produce fixed expectations.
 *
 * This tool resolves each name to the keycode that carries it at level 0 and
 * injects only what it is told, in order.
 *
 * Usage: x11_key_injector [--delay MS] COMMAND...
 *   down NAME     press NAME and leave it down
 *   up NAME       release NAME
 *   tap NAME      press then release NAME
 *   hold NAME MS  press NAME, wait MS with X auto-repeat left as-is, release
 *   sleep MS      wait
 * NAME is any X keysym name (a, shift+... is NOT parsed; press modifiers
 * explicitly with `down Control_L` / `up Control_L`).
 */
#include <X11/Xlib.h>
#include <X11/extensions/XTest.h>
#include <X11/keysym.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static Display *display = NULL;
static unsigned long step_delay_ms = 40;

static void wait_ms(unsigned long milliseconds) {
  struct timespec pause = {
      .tv_sec = (time_t)(milliseconds / 1000),
      .tv_nsec = (long)((milliseconds % 1000) * 1000000L),
  };
  nanosleep(&pause, NULL);
}

static KeyCode resolve(const char *name) {
  KeySym keysym = XStringToKeysym(name);
  if (keysym == NoSymbol) {
    fprintf(stderr, "x11_key_injector: unknown keysym '%s'\n", name);
    exit(2);
  }
  KeyCode keycode = XKeysymToKeycode(display, keysym);
  if (keycode == 0) {
    fprintf(stderr, "x11_key_injector: '%s' is not in the active keymap\n",
            name);
    exit(2);
  }
  // The harness only injects keys the active layout produces unmodified;
  // anything else would need a modifier this tool deliberately never guesses.
  int keysyms_per_keycode = 0;
  KeySym *mapping = XGetKeyboardMapping(display, keycode, 1,
                                        &keysyms_per_keycode);
  bool at_level_zero = mapping && keysyms_per_keycode > 0 &&
                       mapping[0] == keysym;
  if (mapping) XFree(mapping);
  if (!at_level_zero) {
    fprintf(stderr,
            "x11_key_injector: '%s' is not the level-0 symbol of keycode %u\n",
            name, (unsigned)keycode);
    exit(2);
  }
  return keycode;
}

static void inject(const char *name, Bool is_press) {
  XTestFakeKeyEvent(display, resolve(name), is_press, CurrentTime);
  XFlush(display);
}

int main(int argc, char **argv) {
  display = XOpenDisplay(NULL);
  if (!display) {
    fprintf(stderr, "x11_key_injector: cannot open DISPLAY\n");
    return 2;
  }
  int event_base = 0, error_base = 0, major = 0, minor = 0;
  if (!XTestQueryExtension(display, &event_base, &error_base, &major, &minor)) {
    fprintf(stderr, "x11_key_injector: the X server has no XTEST extension\n");
    return 2;
  }

  int index = 1;
  if (index + 1 < argc && strcmp(argv[index], "--delay") == 0) {
    step_delay_ms = strtoul(argv[index + 1], NULL, 10);
    index += 2;
  }

  for (; index < argc; ++index) {
    const char *command = argv[index];
    if (strcmp(command, "sleep") == 0 && index + 1 < argc) {
      wait_ms(strtoul(argv[++index], NULL, 10));
      continue;
    }
    if (strcmp(command, "down") == 0 && index + 1 < argc) {
      inject(argv[++index], True);
    } else if (strcmp(command, "up") == 0 && index + 1 < argc) {
      inject(argv[++index], False);
    } else if (strcmp(command, "tap") == 0 && index + 1 < argc) {
      const char *name = argv[++index];
      inject(name, True);
      wait_ms(step_delay_ms);
      inject(name, False);
    } else if (strcmp(command, "hold") == 0 && index + 2 < argc) {
      const char *name = argv[index + 1];
      unsigned long duration = strtoul(argv[index + 2], NULL, 10);
      index += 2;
      inject(name, True);
      wait_ms(duration);
      inject(name, False);
    } else {
      fprintf(stderr, "x11_key_injector: bad command '%s'\n", command);
      return 2;
    }
    wait_ms(step_delay_ms);
  }
  XCloseDisplay(display);
  return 0;
}
