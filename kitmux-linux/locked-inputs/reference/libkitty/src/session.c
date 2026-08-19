// Session API: each kitty_session wraps one glue.Session (a kitty Screen
// wired to a child on a PTY). All Python access happens between
// PyGILState_Ensure/Release; host callbacks fire after the GIL is released,
// on the thread that called kitty_session_pump.
#include "internal.h"
#include <stdlib.h>
#include <string.h>

static kitty_session *
create_session(kitty_engine *e, int lines, int cols,
               const char *const *argv, const char *cwd,
               const char *const *env,
               const kitty_session_callbacks *cbs,
               char *errbuf, size_t errbuf_len) {
    if (errbuf && errbuf_len) errbuf[0] = 0;
    if (!e) return NULL;
    kitty_session *s = calloc(1, sizeof *s);
    if (!s) return NULL;
    s->e = e;
    s->fd = -1;
    if (cbs) s->cbs = *cbs;

    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *pyargv;
    if (argv && argv[0]) {
        pyargv = PyList_New(0);
        for (int i = 0; pyargv && argv[i]; i++) {
            PyObject *item = PyUnicode_FromString(argv[i]);
            if (!item || PyList_Append(pyargv, item) != 0) Py_CLEAR(pyargv);
            Py_XDECREF(item);
        }
    } else {
        pyargv = Py_None;
        Py_INCREF(pyargv);
    }
    PyObject *pyenv = Py_None;
    Py_INCREF(pyenv);
    if (env) {
        Py_DECREF(pyenv);
        pyenv = PyDict_New();
        for (int i = 0; pyenv && env[i]; i++) {
            const char *equals = strchr(env[i], '=');
            if (!equals || equals == env[i]) continue;
            PyObject *key = PyUnicode_DecodeUTF8(env[i], equals - env[i], "strict");
            PyObject *value = PyUnicode_FromString(equals + 1);
            if (!key || !value || PyDict_SetItem(pyenv, key, value) != 0) {
                Py_CLEAR(pyenv);
            }
            Py_XDECREF(key);
            Py_XDECREF(value);
        }
    }
    if (pyargv && pyenv) {
        s->py = PyObject_CallMethod(e->glue, "Session", "iiOzO",
                                    lines, cols, pyargv, cwd, pyenv);
    }
    Py_XDECREF(pyargv);
    Py_XDECREF(pyenv);
    if (s->py) {
        PyObject *fd = PyObject_CallMethod(s->py, "fileno", NULL);
        if (fd) { s->fd = (int)PyLong_AsLong(fd); Py_DECREF(fd); }
    }
    if (!s->py || s->fd < 0) {
        lk_format_py_error(errbuf, errbuf_len);
        Py_XDECREF(s->py);
        PyGILState_Release(g);
        free(s);
        return NULL;
    }
    PyGILState_Release(g);
    return s;
}

kitty_session *
kitty_session_create(kitty_engine *e, int lines, int cols,
                     const char *const *argv,
                     const kitty_session_callbacks *cbs,
                     char *errbuf, size_t errbuf_len) {
    return create_session(e, lines, cols, argv, NULL, NULL,
                          cbs, errbuf, errbuf_len);
}

kitty_session *
kitty_session_create_with_cwd(kitty_engine *e, int lines, int cols,
                              const char *const *argv, const char *cwd,
                              const kitty_session_callbacks *cbs,
                              char *errbuf, size_t errbuf_len) {
    return create_session(e, lines, cols, argv, cwd, NULL,
                          cbs, errbuf, errbuf_len);
}

kitty_session *
kitty_session_create_with_options(kitty_engine *e, int lines, int cols,
                                  const char *const *argv, const char *cwd,
                                  const char *const *env,
                                  const kitty_session_callbacks *cbs,
                                  char *errbuf, size_t errbuf_len) {
    return create_session(e, lines, cols, argv, cwd, env,
                          cbs, errbuf, errbuf_len);
}

void
kitty_session_close(kitty_session *s) {
    if (!s) return;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "close", NULL);
    if (!r) PyErr_Print(); else Py_DECREF(r);
    Py_DECREF(s->py);
    PyGILState_Release(g);
    free(s);
}

bool
kitty_session_child_alive(kitty_session *s) {
    if (!s) return false;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *a = PyObject_GetAttrString(s->py, "child_alive");
    bool alive = a && PyObject_IsTrue(a);
    Py_XDECREF(a);
    PyGILState_Release(g);
    return alive;
}

int
kitty_session_child_pid(kitty_session *s) {
    if (!s) return -1;
    int out = -1;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "child_pid_value", NULL);
    if (r) { out = (int)PyLong_AsLong(r); Py_DECREF(r); }
    if (PyErr_Occurred()) { PyErr_Clear(); out = -1; }
    PyGILState_Release(g);
    return out;
}

bool
kitty_session_has_foreground_process(kitty_session *s) {
    if (!s) return false;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "has_foreground_process", NULL);
    bool foreground = r && PyObject_IsTrue(r) > 0;
    if (!r || PyErr_Occurred()) PyErr_Clear();
    Py_XDECREF(r);
    PyGILState_Release(g);
    return foreground;
}

int
kitty_session_fd(kitty_session *s) {
    return s ? s->fd : -1;
}

bool
kitty_session_pump(kitty_session *s) {
    if (!s) return false;
    bool changed = false, exited = false;
    s->last_pump_bytes = 0;
    char *title = NULL;
    long bells = 0, status = 0;
    struct { char *title; char *body; } *notes = NULL;
    Py_ssize_t nnotes = 0;
    struct { char *key; char *value; } *uvars = NULL;
    Py_ssize_t nuvars = 0;

    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "pump", "d", 0.0);
    if (r && PyTuple_Check(r) && PyTuple_GET_SIZE(r) >= 4) {
        changed = PyObject_IsTrue(PyTuple_GET_ITEM(r, 0));
        PyObject *t = PyTuple_GET_ITEM(r, 1);
        if (t != Py_None) {
            const char *u = PyUnicode_AsUTF8(t);
            if (u) title = strdup(u);
        }
        bells = PyLong_AsLong(PyTuple_GET_ITEM(r, 2));
        PyObject *st = PyTuple_GET_ITEM(r, 3);
        if (st != Py_None) { exited = true; status = PyLong_AsLong(st); }
        // v0.9: fifth element is a list of (title, body) notification pairs.
        // Copy them out under the GIL; callbacks fire after release, per the
        // file contract.
        if (PyTuple_GET_SIZE(r) >= 5) {
            PyObject *lst = PyTuple_GET_ITEM(r, 4);
            if (PyList_Check(lst) && PyList_GET_SIZE(lst) > 0) {
                Py_ssize_t count = PyList_GET_SIZE(lst);
                notes = calloc((size_t)count, sizeof *notes);
                for (Py_ssize_t i = 0; notes && i < count; i++) {
                    PyObject *item = PyList_GET_ITEM(lst, i);
                    if (!PyTuple_Check(item) || PyTuple_GET_SIZE(item) != 2) continue;
                    const char *nt = PyUnicode_AsUTF8(PyTuple_GET_ITEM(item, 0));
                    const char *nb = PyUnicode_AsUTF8(PyTuple_GET_ITEM(item, 1));
                    if (!nt || !nb) { PyErr_Clear(); continue; }
                    notes[nnotes].title = strdup(nt);
                    notes[nnotes].body = strdup(nb);
                    if (!notes[nnotes].title || !notes[nnotes].body) {
                        free(notes[nnotes].title);
                        free(notes[nnotes].body);
                        continue;
                    }
                    nnotes++;
                }
            }
        }
        // v0.11: sixth element is a list of (key, value) user-var pairs.
        if (PyTuple_GET_SIZE(r) >= 6) {
            PyObject *lst = PyTuple_GET_ITEM(r, 5);
            if (PyList_Check(lst) && PyList_GET_SIZE(lst) > 0) {
                Py_ssize_t count = PyList_GET_SIZE(lst);
                uvars = calloc((size_t)count, sizeof *uvars);
                for (Py_ssize_t i = 0; uvars && i < count; i++) {
                    PyObject *item = PyList_GET_ITEM(lst, i);
                    if (!PyTuple_Check(item) || PyTuple_GET_SIZE(item) != 2) continue;
                    const char *uk = PyUnicode_AsUTF8(PyTuple_GET_ITEM(item, 0));
                    const char *uv = PyUnicode_AsUTF8(PyTuple_GET_ITEM(item, 1));
                    if (!uk || !uv) { PyErr_Clear(); continue; }
                    uvars[nuvars].key = strdup(uk);
                    uvars[nuvars].value = strdup(uv);
                    if (!uvars[nuvars].key || !uvars[nuvars].value) {
                        free(uvars[nuvars].key);
                        free(uvars[nuvars].value);
                        continue;
                    }
                    nuvars++;
                }
            }
        }
        // Additive verification metric: glue reports bytes consumed by this
        // bounded pump turn as the seventh tuple element.
        if (PyTuple_GET_SIZE(r) >= 7) {
            unsigned long long value = PyLong_AsUnsignedLongLong(
                PyTuple_GET_ITEM(r, 6));
            if (!PyErr_Occurred()) s->last_pump_bytes = (size_t)value;
            else PyErr_Clear();
        }
    } else if (!r) {
        PyErr_Print();
    }
    Py_XDECREF(r);
    PyGILState_Release(g);

    if (changed && s->cbs.on_damage) s->cbs.on_damage(s->cbs.userdata);
    if (title) {
        if (s->cbs.on_title) s->cbs.on_title(s->cbs.userdata, title);
        free(title);
    }
    if (s->cbs.on_bell)
        for (long i = 0; i < bells; i++) s->cbs.on_bell(s->cbs.userdata);
    for (Py_ssize_t i = 0; i < nnotes; i++) {
        if (s->cbs.on_notification)
            s->cbs.on_notification(s->cbs.userdata, notes[i].title, notes[i].body);
        free(notes[i].title);
        free(notes[i].body);
    }
    free(notes);
    for (Py_ssize_t i = 0; i < nuvars; i++) {
        if (s->cbs.on_user_var)
            s->cbs.on_user_var(s->cbs.userdata, uvars[i].key, uvars[i].value);
        free(uvars[i].key);
        free(uvars[i].value);
    }
    free(uvars);
    if (exited && !s->exit_reported) {
        s->exit_reported = true;
        if (s->cbs.on_child_exit) s->cbs.on_child_exit(s->cbs.userdata, (int)status);
    }
    return changed;
}

size_t
kitty_session_last_pump_bytes(kitty_session *s) {
    return s ? s->last_pump_bytes : 0;
}

void
kitty_session_write(kitty_session *s, const uint8_t *data, size_t len) {
    if (!s || !data || !len) return;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "write_to_child", "y#", (const char *)data, (Py_ssize_t)len);
    if (!r) PyErr_Print(); else Py_DECREF(r);
    PyGILState_Release(g);
}

void
kitty_session_paste(kitty_session *s, const uint8_t *data, size_t len) {
    if (!s || !data || !len) return;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "paste", "y#", (const char *)data, (Py_ssize_t)len);
    if (!r) PyErr_Print(); else Py_DECREF(r);
    PyGILState_Release(g);
}

void
kitty_session_scroll(kitty_session *s, int lines) {
    if (!s || !lines) return;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "scroll", "i", lines);
    if (!r) PyErr_Print(); else Py_DECREF(r);
    PyGILState_Release(g);
}

void
kitty_session_clear_scrollback(kitty_session *s) {
    if (!s) return;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "clear_scrollback", NULL);
    if (!r) PyErr_Print(); else Py_DECREF(r);
    PyGILState_Release(g);
}

unsigned int
kitty_session_scrolled_by(kitty_session *s) {
    if (!s) return 0;
    unsigned int out = 0;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "scrolled_by", NULL);
    if (r) {
        unsigned long v = PyLong_AsUnsignedLong(r);
        if (!PyErr_Occurred()) out = (unsigned int)v;
        else PyErr_Print();
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    return out;
}

unsigned int
kitty_session_history_line_count(kitty_session *s) {
    if (!s) return 0;
    unsigned int out = 0;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "history_line_count", NULL);
    if (r) {
        unsigned long v = PyLong_AsUnsignedLong(r);
        if (!PyErr_Occurred()) out = (unsigned int)v; else PyErr_Clear();
        Py_DECREF(r);
    } else PyErr_Clear();
    PyGILState_Release(g);
    return out;
}

bool
kitty_session_cursor_cell(kitty_session *s, int *x, int *y, bool *visible) {
    if (!s) return false;
    bool ok = false;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "cursor_cell", NULL);
    if (r && PyTuple_Check(r) && PyTuple_GET_SIZE(r) == 3) {
        long cx = PyLong_AsLong(PyTuple_GET_ITEM(r, 0));
        long cy = PyLong_AsLong(PyTuple_GET_ITEM(r, 1));
        int vis = PyObject_IsTrue(PyTuple_GET_ITEM(r, 2));
        if (!PyErr_Occurred() && vis >= 0) {
            if (x) *x = (int)cx;
            if (y) *y = (int)cy;
            if (visible) *visible = vis == 1;
            ok = true;
        } else {
            PyErr_Print();
        }
    }
    if (!r) PyErr_Print();
    Py_XDECREF(r);
    PyGILState_Release(g);
    return ok;
}

void
kitty_session_selection_start(kitty_session *s, unsigned int column,
                              unsigned int row, bool in_left_half_of_cell,
                              unsigned int extend_mode) {
    if (!s) return;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(
        s->py, "selection_start", "IIpI",
        column, row, in_left_half_of_cell, extend_mode);
    if (!r) PyErr_Print(); else Py_DECREF(r);
    PyGILState_Release(g);
}

void
kitty_session_selection_update(kitty_session *s, unsigned int column,
                               unsigned int row, bool in_left_half_of_cell,
                               bool ended) {
    if (!s) return;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(
        s->py, "selection_update", "IIpp",
        column, row, in_left_half_of_cell, ended);
    if (!r) PyErr_Print(); else Py_DECREF(r);
    PyGILState_Release(g);
}

void
kitty_session_selection_clear(kitty_session *s) {
    if (!s) return;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "selection_clear", NULL);
    if (!r) PyErr_Print(); else Py_DECREF(r);
    PyGILState_Release(g);
}

char *
kitty_session_selection_text(kitty_session *s) {
    if (!s) return NULL;
    char *out = NULL;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "selection_text", NULL);
    if (r) {
        const char *u = PyUnicode_AsUTF8(r);
        if (u) out = strdup(u);
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    return out;
}

size_t
kitty_session_search_set(kitty_session *s, const char *query, size_t query_len) {
    size_t count = 0;
    (void)kitty_session_search_set_options(
        s, query, query_len, false, false, &count, NULL, 0);
    return count;
}

bool
kitty_session_search_set_options(kitty_session *s,
                                 const char *query, size_t query_len,
                                 bool case_sensitive, bool regex,
                                 size_t *match_count,
                                 char *errbuf, size_t errbuf_len) {
    if (match_count) *match_count = 0;
    if (errbuf && errbuf_len) errbuf[0] = 0;
    if (!s || (!query && query_len)) return false;
    bool valid = false;
    size_t count = 0;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(
        s->py, "search_set", "y#pp",
        query ? query : "", (Py_ssize_t)query_len,
        case_sensitive, regex);
    if (r) {
        PyObject *count_obj = PyTuple_Check(r) && PyTuple_Size(r) == 2
            ? PyTuple_GetItem(r, 0) : NULL;
        PyObject *error_obj = count_obj ? PyTuple_GetItem(r, 1) : NULL;
        if (count_obj && error_obj) {
            unsigned long long v = PyLong_AsUnsignedLongLong(count_obj);
            const char *error = PyUnicode_Check(error_obj)
                ? PyUnicode_AsUTF8(error_obj) : NULL;
            if (!PyErr_Occurred() && error) {
                count = (size_t)v;
                valid = error[0] == 0;
                if (!valid && errbuf && errbuf_len) {
                    strncpy(errbuf, error, errbuf_len - 1);
                    errbuf[errbuf_len - 1] = 0;
                }
            } else {
                PyErr_Print();
            }
        }
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    if (match_count) *match_count = count;
    return valid;
}

bool
kitty_session_search_next(kitty_session *s, bool backwards) {
    if (!s) return false;
    bool found = false;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "search_next", "p", backwards);
    if (r) {
        found = PyObject_IsTrue(r);
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    return found;
}

size_t
kitty_session_search_refresh(kitty_session *s) {
    if (!s) return 0;
    size_t count = 0;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "search_refresh", NULL);
    if (r) {
        unsigned long long v = PyLong_AsUnsignedLongLong(r);
        if (!PyErr_Occurred()) count = (size_t)v;
        else PyErr_Print();
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    return count;
}

size_t
kitty_session_search_visible_mark_count(kitty_session *s, unsigned int mark) {
    if (!s) return 0;
    size_t count = 0;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "search_visible_mark_count", "I", mark);
    if (r) {
        unsigned long long v = PyLong_AsUnsignedLongLong(r);
        if (!PyErr_Occurred()) count = (size_t)v;
        else PyErr_Print();
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    return count;
}

void
kitty_session_search_clear(kitty_session *s) {
    if (!s) return;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "search_clear", NULL);
    if (!r) PyErr_Print(); else Py_DECREF(r);
    PyGILState_Release(g);
}

int
kitty_session_mouse_event(kitty_session *s,
                          unsigned int cell_x, unsigned int cell_y,
                          int button, int action, uint32_t mods,
                          int pixel_x, int pixel_y) {
    if (!s) return 0;
    int handled = 0;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(
        s->py, "mouse_event", "IIiiIii",
        cell_x, cell_y, button, action, mods, pixel_x, pixel_y);
    if (r) {
        handled = PyObject_IsTrue(r);
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    return handled;
}

char *
kitty_session_reported_cwd(kitty_session *s) {
    if (!s) return NULL;
    char *out = NULL;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "reported_cwd", NULL);
    if (r) {
        const char *u = PyUnicode_AsUTF8(r);
        if (u) out = strdup(u);
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    return out;
}

void
kitty_session_resize(kitty_session *s, int lines, int cols) {
    if (!s) return;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "resize", "ii", lines, cols);
    if (!r) PyErr_Print(); else Py_DECREF(r);
    PyGILState_Release(g);
}

size_t
kitty_session_encode_key(kitty_session *s, const kitty_key_event *ev,
                         char *out, size_t out_len) {
    if (!s || !ev || !out || !out_len) return 0;
    size_t written = 0;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(
        s->e->glue, "encode_key", "OIIIIis",
        s->py, ev->key, ev->shifted_key, ev->alternate_key, ev->mods,
        ev->action, ev->text ? ev->text : "");
    if (r) {
        char *buf = NULL; Py_ssize_t n = 0;
        if (PyBytes_AsStringAndSize(r, &buf, &n) == 0 && n > 0 && (size_t)n <= out_len) {
            memcpy(out, buf, (size_t)n);
            written = (size_t)n;
        }
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    return written;
}

char *
kitty_session_text(kitty_session *s) {
    if (!s) return NULL;
    char *out = NULL;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "text", NULL);
    if (r) {
        const char *u = PyUnicode_AsUTF8(r);
        if (u) out = strdup(u);
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    return out;
}

size_t
kitty_session_line_wraps(kitty_session *s, uint8_t *out, size_t capacity) {
    if (!s || !out || !capacity) return 0;
    size_t written = 0;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(s->py, "line_wraps", NULL);
    if (r) {
        char *buf = NULL;
        Py_ssize_t n = 0;
        if (PyBytes_AsStringAndSize(r, &buf, &n) == 0 && n > 0) {
            written = (size_t)n < capacity ? (size_t)n : capacity;
            memcpy(out, buf, written);
        }
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    return written;
}

char *
kitty_session_scrollback_text(kitty_session *s,
                              size_t max_lines, size_t max_bytes) {
    if (!s || max_lines > PY_SSIZE_T_MAX || max_bytes > PY_SSIZE_T_MAX) return NULL;
    char *out = NULL;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(
        s->py, "scrollback_text", "nn",
        (Py_ssize_t)max_lines, (Py_ssize_t)max_bytes);
    if (r) {
        const char *u = PyUnicode_AsUTF8(r);
        if (u) out = strdup(u);
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    return out;
}

bool
kitty_session_replay_plain_text(kitty_session *s,
                                const uint8_t *data, size_t len) {
    if (!s || (!data && len) || len > PY_SSIZE_T_MAX) return false;
    bool changed = false;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(
        s->py, "replay_plain_text", "y#",
        data ? (const char *)data : "", (Py_ssize_t)len);
    if (r) {
        changed = PyObject_IsTrue(r);
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    return changed;
}
