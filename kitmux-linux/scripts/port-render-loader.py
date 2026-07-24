#!/usr/bin/env python3
"""Apply the bounded Linux delta to the tagged macOS render-export shim."""

from pathlib import Path
import sys


path = Path(sys.argv[1])
source = path.read_text()

if "libkitty_gl_loader" in source and 'dlopen("libGL.so.1"' in source:
    print("already applied: Linux libkitty GL loader")
    raise SystemExit(0)

replacements = {
    """#include <dlfcn.h>

extern PyTypeObject Screen_Type;

static GLADapiproc
opengl_framework_loader(const char *name) {
    static void *handle = NULL;
    if (!handle) {
        handle = dlopen("/System/Library/Frameworks/OpenGL.framework/Versions/Current/OpenGL", RTLD_LAZY | RTLD_GLOBAL);
        if (!handle) handle = dlopen("/System/Library/Frameworks/OpenGL.framework/OpenGL", RTLD_LAZY | RTLD_GLOBAL);
    }
    return handle ? (GLADapiproc)dlsym(handle, name) : NULL;
}
""": """#include <dlfcn.h>
#include <string.h>

extern PyTypeObject Screen_Type;

static GLADapiproc
symbol_to_gl_proc(void *symbol) {
    GLADapiproc proc = NULL;
    _Static_assert(sizeof proc == sizeof symbol, "GL function and data pointers differ");
    memcpy(&proc, &symbol, sizeof proc);
    return proc;
}

static GLADapiproc
libkitty_gl_loader(const char *name) {
    static void *handle = NULL;
    if (!handle) {
#ifdef __APPLE__
        handle = dlopen("/System/Library/Frameworks/OpenGL.framework/Versions/Current/OpenGL", RTLD_LAZY | RTLD_GLOBAL);
        if (!handle) handle = dlopen("/System/Library/Frameworks/OpenGL.framework/OpenGL", RTLD_LAZY | RTLD_GLOBAL);
#else
        handle = dlopen("libGL.so.1", RTLD_LAZY | RTLD_GLOBAL);
#endif
    }
    if (!handle) return NULL;

    void *symbol = dlsym(handle, name);
    if (symbol) return symbol_to_gl_proc(symbol);

#ifndef __APPLE__
    typedef GLADapiproc (*glx_get_proc_address)(const unsigned char *);
    static glx_get_proc_address get_proc = NULL;
    if (!get_proc) {
        void *resolver = dlsym(handle, "glXGetProcAddressARB");
        _Static_assert(sizeof get_proc == sizeof resolver,
                       "GLX resolver and data pointers differ");
        memcpy(&get_proc, &resolver, sizeof get_proc);
    }
    if (get_proc) return get_proc((const unsigned char *)name);
#endif
    return NULL;
}
""",
    """    // Mirrors gl_init() (gl.c) but loads GL from the OpenGL framework
    // directly instead of via glfwGetProcAddress. Requires a current context.
""": """    // Mirrors gl_init() (gl.c) without depending on a GLFW-owned window.
    // The embedding host must make its GL context current first.
""",
    "global_state.gl_version = gladLoadGL(opengl_framework_loader);":
        "global_state.gl_version = gladLoadGL(libkitty_gl_loader);",
    "        global_state.supports_framebuffer_srgb = true;  // always true on macOS, see gl.c\n":
        """#ifdef __APPLE__
        global_state.supports_framebuffer_srgb = true;
#else
        global_state.supports_framebuffer_srgb =
            (GLAD_GL_ARB_framebuffer_sRGB + GLAD_GL_EXT_framebuffer_sRGB) != 0;
#endif
""",
    '"Load GL via the macOS OpenGL framework; requires a current context"':
        '"Load GL from the host platform; requires a current context"',
}

for old, new in replacements.items():
    if source.count(old) != 1:
        raise SystemExit(f"expected one render-loader source fragment, found {source.count(old)}")
    source = source.replace(old, new)

path.write_text(source)
print("applied Linux libkitty GL loader")

