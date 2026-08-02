#include "libkitty.h"
#include "gtk_key_translation.h"

#include <stddef.h>

size_t libkitty_engine_config_size(void) {
    return sizeof(kitty_engine_config);
}

size_t libkitty_session_callbacks_size(void) {
    return sizeof(kitty_session_callbacks);
}

size_t kitmux_gdk_key_input_size(void) {
    return sizeof(kitmux_gdk_key_input);
}

size_t kitmux_key_translation_size(void) {
    return sizeof(kitmux_key_translation);
}

size_t kitmux_key_tracker_size(void) {
    return sizeof(kitmux_key_tracker);
}
