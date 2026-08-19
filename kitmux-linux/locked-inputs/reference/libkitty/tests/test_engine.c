#include "libkitty.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    char err[512] = "";
    kitty_engine_config cfg = {
        .kitty_src_path = getenv("KITTY_SRC"),
        .libkitty_py_path = getenv("LIBKITTY_PY"),
    };
    kitty_engine *e = kitty_engine_init(&cfg, err, sizeof err);
    if (!e) { fprintf(stderr, "init failed: %s\n", err); return 1; }
    assert(kitty_render_font_size(e) == 0.0);
    kitty_engine_shutdown(e);
    printf("engine lifecycle OK\n");
    return 0;
}
