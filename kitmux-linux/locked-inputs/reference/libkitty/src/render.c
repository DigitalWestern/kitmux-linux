// Render API: kitty's own GL pipeline (shaders, font atlas, draw_cells)
// painting into a framebuffer the host owns. Requires the additive exports
// patch (patches/0001-libkitty-render-exports.patch) in the kitty build.
// Every function here must be called with the target GL context current.
#include "internal.h"
#include <math.h>
#include <stdlib.h>

bool
kitty_render_init(kitty_engine *e, double scale,
                  int *cell_width_px, int *cell_height_px,
                  char *errbuf, size_t errbuf_len) {
    if (errbuf && errbuf_len) errbuf[0] = 0;
    if (!e) return false;
    if (e->render_ready) {
        if (cell_width_px) *cell_width_px = e->cell_w;
        if (cell_height_px) *cell_height_px = e->cell_h;
        return true;
    }
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(e->glue, "render_init", "d", scale);
    bool ok = false;
    if (r && PyTuple_Check(r) && PyTuple_GET_SIZE(r) == 3) {
        e->font_size = PyFloat_AsDouble(PyTuple_GET_ITEM(r, 0));
        e->cell_w = (int)PyLong_AsLong(PyTuple_GET_ITEM(r, 1));
        e->cell_h = (int)PyLong_AsLong(PyTuple_GET_ITEM(r, 2));
        e->render_ready = ok = true;
    } else if (!r) {
        lk_format_py_error(errbuf, errbuf_len);
    }
    Py_XDECREF(r);
    PyGILState_Release(g);
    if (ok) {
        if (cell_width_px) *cell_width_px = e->cell_w;
        if (cell_height_px) *cell_height_px = e->cell_h;
    }
    return ok;
}

double
kitty_render_font_size(kitty_engine *e) {
    return e && e->render_ready ? e->font_size : 0.0;
}

bool
kitty_render_set_font_size(kitty_engine *e, double font_size_points,
                           int *cell_width_px, int *cell_height_px,
                           char *errbuf, size_t errbuf_len) {
    if (errbuf && errbuf_len) errbuf[0] = 0;
    if (!e || !e->render_ready) {
        if (errbuf && errbuf_len) snprintf(errbuf, errbuf_len, "renderer is not initialized");
        return false;
    }
    if (!isfinite(font_size_points) || font_size_points < 4.0) {
        if (errbuf && errbuf_len) snprintf(errbuf, errbuf_len, "font size must be finite and at least 4 points");
        return false;
    }
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(e->glue, "render_set_font_size", "d", font_size_points);
    bool ok = false;
    if (r && PyTuple_Check(r) && PyTuple_GET_SIZE(r) == 2) {
        int cell_w = (int)PyLong_AsLong(PyTuple_GET_ITEM(r, 0));
        int cell_h = (int)PyLong_AsLong(PyTuple_GET_ITEM(r, 1));
        if (!PyErr_Occurred() && cell_w > 0 && cell_h > 0) {
            e->font_size = font_size_points;
            e->cell_w = cell_w;
            e->cell_h = cell_h;
            ok = true;
        }
    }
    if (!ok && PyErr_Occurred()) lk_format_py_error(errbuf, errbuf_len);
    else if (!ok && errbuf && errbuf_len) snprintf(errbuf, errbuf_len, "invalid font metrics");
    Py_XDECREF(r);
    PyGILState_Release(g);
    if (ok) {
        if (cell_width_px) *cell_width_px = e->cell_w;
        if (cell_height_px) *cell_height_px = e->cell_h;
    }
    return ok;
}

bool
kitty_session_set_viewport(kitty_session *s, int fb_width_px, int fb_height_px) {
    return kitty_session_set_geometry(s, fb_width_px, fb_height_px,
                                      0, 0, fb_width_px, fb_height_px);
}

bool
kitty_session_set_geometry(kitty_session *s, int fb_width_px, int fb_height_px,
                           int left, int top, int right, int bottom) {
    if (!s || !s->e->render_ready || fb_width_px <= 0 || fb_height_px <= 0) return false;
    if (right <= left || bottom <= top) return false;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->e->glue, "set_geometry", "Oiiiiii",
                                      s->py, fb_width_px, fb_height_px, left, top, right, bottom);
    bool ok = r != NULL;
    if (!r) PyErr_Print(); else Py_DECREF(r);
    PyGILState_Release(g);
    if (ok) s->has_viewport = true;
    return ok;
}

bool
kitty_session_release_render_resources(kitty_session *s) {
    // Caller contract (see libkitty.h): the GL context that created the VAO
    // must be current. Clearing has_viewport makes the draw entry points fail
    // closed until set_geometry rebuilds the VAO in the destination context,
    // rather than letting a draw run against a released one.
    if (!s || !s->e->render_ready) return false;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->e->glue, "release_render_resources",
                                      "O", s->py);
    bool ok = r != NULL;
    if (!r) PyErr_Print(); else Py_DECREF(r);
    PyGILState_Release(g);
    if (ok) s->has_viewport = false;
    return ok;
}

bool
kitty_session_draw(kitty_session *s) {
    return kitty_session_draw_with_state(s, true, true, true, true);
}

bool
kitty_session_draw_with_cursor_visibility(kitty_session *s,
                                          bool cursor_visible) {
    return kitty_session_draw_with_state(s, cursor_visible, true, true, true);
}

bool
kitty_session_draw_with_state(kitty_session *s,
                              bool cursor_visible,
                              bool pane_active,
                              bool window_focused,
                              bool single_pane) {
    if (!s || !s->has_viewport) return false;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(
        s->e->glue, "draw", "Oiiii", s->py,
        cursor_visible ? 1 : 0,
        pane_active ? 1 : 0,
        window_focused ? 1 : 0,
        single_pane ? 1 : 0);
    bool ok = r != NULL;
    if (!r) PyErr_Print(); else Py_DECREF(r);
    PyGILState_Release(g);
    return ok;
}
