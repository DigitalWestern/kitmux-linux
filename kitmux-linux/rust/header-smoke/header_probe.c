#include "libkitty.h"

#include <stddef.h>

size_t libkitty_engine_config_size(void) {
    return sizeof(kitty_engine_config);
}

size_t libkitty_session_callbacks_size(void) {
    return sizeof(kitty_session_callbacks);
}
