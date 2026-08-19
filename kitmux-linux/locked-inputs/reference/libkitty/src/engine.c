// Engine lifecycle: owns the embedded CPython interpreter. Established in
// Phase 2 (docs/event-loop-findings.md): the host process/run loop stays in
// charge; the interpreter is a passive subsystem entered per-call via
// PyGILState_Ensure and left via PyGILState_Release.
#include "internal.h"
#include <stdlib.h>
#include <string.h>

void
lk_format_py_error(char *errbuf, size_t errbuf_len) {
    PyObject *exc = PyErr_GetRaisedException();
    if (!exc) {
        if (errbuf && errbuf_len) snprintf(errbuf, errbuf_len, "unknown error (no Python exception)");
        return;
    }
    if (!errbuf || !errbuf_len) {
        PyErr_SetRaisedException(exc);
        PyErr_Print();
        return;
    }
    PyObject *str = PyObject_Str(exc);
    const char *msg = str ? PyUnicode_AsUTF8(str) : NULL;
    snprintf(errbuf, errbuf_len, "%s: %s", Py_TYPE(exc)->tp_name, msg ? msg : "<unprintable>");
    Py_XDECREF(str);
    Py_DECREF(exc);
}

static bool
prepend_sys_path(const char *path) {
    PyObject *sys_path = PySys_GetObject("path");  // borrowed
    if (!sys_path) return false;
    PyObject *p = PyUnicode_FromString(path);
    if (!p) return false;
    int rc = PyList_Insert(sys_path, 0, p);
    Py_DECREF(p);
    return rc == 0;
}

kitty_engine *
kitty_engine_init(const kitty_engine_config *cfg, char *errbuf, size_t errbuf_len) {
    if (errbuf && errbuf_len) errbuf[0] = 0;
    if (!cfg || !cfg->kitty_src_path || !cfg->libkitty_py_path) {
        if (errbuf && errbuf_len) snprintf(errbuf, errbuf_len, "kitty_src_path and libkitty_py_path are required");
        return NULL;
    }
    if (Py_IsInitialized()) {
        if (errbuf && errbuf_len) snprintf(errbuf, errbuf_len, "an engine already exists in this process");
        return NULL;
    }

    PyConfig config;
    PyConfig_InitPythonConfig(&config);
    config.install_signal_handlers = 0;  // the host owns signal handling
    config.parse_argv = 0;
    if (cfg->python_home) {
        // A packaged app owns its interpreter inputs. Ignore ambient
        // PYTHONHOME/PYTHONPATH so another Python installation cannot leak in.
        config.use_environment = 0;
        // The runtime lives inside the signed app. Writing import caches there
        // would invalidate the code signature after the first launch.
        config.write_bytecode = 0;
        PyConfig_SetBytesString(&config, &config.home, cfg->python_home);
    }
    PyStatus st = Py_InitializeFromConfig(&config);
    PyConfig_Clear(&config);
    if (PyStatus_Exception(st)) {
        if (errbuf && errbuf_len)
            snprintf(errbuf, errbuf_len, "Py_InitializeFromConfig: %s", st.err_msg ? st.err_msg : "unknown");
        return NULL;
    }

    kitty_engine *e = calloc(1, sizeof *e);
    if (!e) { Py_FinalizeEx(); return NULL; }

    if (!prepend_sys_path(cfg->kitty_src_path) || !prepend_sys_path(cfg->libkitty_py_path))
        goto fail;
    e->glue = PyImport_ImportModule("glue");
    if (!e->glue) goto fail;
    PyObject *r = PyObject_CallMethod(e->glue, "init", "z", cfg->config_path);
    if (!r) goto fail;
    Py_DECREF(r);

    e->tstate = PyEval_SaveThread();
    return e;

fail:
    lk_format_py_error(errbuf, errbuf_len);
    Py_XDECREF(e->glue);
    Py_FinalizeEx();
    free(e);
    return NULL;
}

int
kitty_engine_option_as_alt(kitty_engine *e) {
    if (!e) return 0;
    int v = 0;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(e->glue, "option_as_alt", NULL);
    if (r) {
        v = (int)PyLong_AsLong(r);
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    return v;
}

bool
kitty_engine_config_number(kitty_engine *e, const char *key, double *out) {
    if (!e || !key || !out) return false;
    bool ok = false;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(e->glue, "config_number", "s", key);
    if (r) {
        if (r != Py_None) {
            double value = PyFloat_AsDouble(r);
            if (!PyErr_Occurred()) {
                *out = value;
                ok = true;
            } else {
                PyErr_Print();
            }
        }
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    return ok;
}

char *
kitty_engine_config_string(kitty_engine *e, const char *key) {
    if (!e || !key) return NULL;
    char *copy = NULL;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(e->glue, "config_string", "s", key);
    if (r) {
        if (r != Py_None) {
            const char *value = PyUnicode_AsUTF8(r);
            if (value) copy = strdup(value);
            else PyErr_Print();
        }
        Py_DECREF(r);
    } else {
        PyErr_Print();
    }
    PyGILState_Release(g);
    return copy;
}

int
kitty_engine_reload_config(kitty_engine *e, const char *config_path,
                           char *errbuf, size_t errbuf_len) {
    if (errbuf && errbuf_len) errbuf[0] = 0;
    if (!e) return -1;
    int patched = -1;
    PyGILState_STATE g = PyGILState_Ensure();
    PyObject *r = PyObject_CallMethod(e->glue, "reload_config", "z", config_path);
    if (r) {
        patched = (int)PyLong_AsLong(r);
        Py_DECREF(r);
    } else {
        lk_format_py_error(errbuf, errbuf_len);
    }
    PyGILState_Release(g);
    return patched;
}

void
kitty_engine_shutdown(kitty_engine *e) {
    if (!e) return;
    PyEval_RestoreThread(e->tstate);
    Py_XDECREF(e->glue);
    Py_FinalizeEx();
    free(e);
}
