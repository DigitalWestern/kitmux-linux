// Shared internals for libkitty's C layer. Not installed; hosts see only
// include/libkitty.h.
#ifndef LIBKITTY_INTERNAL_H
#define LIBKITTY_INTERNAL_H

#define PY_SSIZE_T_CLEAN
#include <Python.h>
#include "libkitty.h"

struct kitty_engine {
    PyThreadState *tstate;   // main thread state, saved so the host owns no GIL by default
    PyObject *glue;          // the glue module
    bool render_ready;
    int cell_w, cell_h;      // pixels, valid once render_ready
    double font_size;        // points, valid once render_ready
};

struct kitty_session {
    kitty_engine *e;
    PyObject *py;            // glue.Session instance
    int fd;                  // master PTY fd, cached at create
    kitty_session_callbacks cbs;
    bool exit_reported;
    bool has_viewport;
    size_t last_pump_bytes;
};

// Formats the pending Python exception into errbuf (clears it), or prints it
// to stderr if errbuf is NULL/empty-sized. Safe to call with no error set.
void lk_format_py_error(char *errbuf, size_t errbuf_len);

#endif
