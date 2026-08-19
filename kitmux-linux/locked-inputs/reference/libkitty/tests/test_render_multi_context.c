// Multi-window prerequisite gate (Kitmux sub-project C, Task 0).
//
// Proves that a session's GPU draw state can move between two EXPLICITLY
// SHARED GL contexts, which is what "a workspace displays in one window at a
// time but may move between windows" requires of the renderer.
//
// Why this is a C test and not an AppKit smoke: everything under test is
// context lifetime, not UI. CGLCreateContext's second argument is the share
// context, so two shared contexts are reachable headlessly and deterministically.
//
// The sentinel is a second session living in the other context throughout, so
// every stage checks that operating on one session leaves the other context's
// resources intact.
//
// WHAT THIS TEST DOES AND DOES NOT PROVE. It proves the correct lifecycle
// works end to end: release under the source context, rebuild under the
// destination, repeatedly, across geometry and font changes and context
// teardown. It does NOT prove the ordering is enforced. Two deliberate probes
// were run during development and BOTH still passed:
//
//   1. skipping the release entirely on the A->B transfer;
//   2. performing the release with the WRONG context current.
//
// Neither is safe. Per the GL spec, VAO names live in per-context namespaces
// while kitty's vaos[] registry is process-global, so a name collision lets one
// context delete or reuse another's VAO. The probes passed only because this
// particular run allocated non-colliding names, and because cell data is
// re-uploaded every frame, so cross-session buffer corruption self-heals before
// the next glReadPixels. The host must honour the header contract on
// kitty_session_release_render_resources; this gate cannot police it.
#define GL_SILENCE_DEPRECATION
#include "libkitty.h"
#include <OpenGL/OpenGL.h>
#include <OpenGL/gl3.h>
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

// Deliberately different sizes: a transfer must survive a geometry change, and
// differing dimensions catch a stub that kept the previous window's viewport.
#define A_W 800
#define A_H 600
#define B_W 640
#define B_H 480

// Per-context clear colors, so a frame read back from the wrong FBO is obvious
// rather than merely wrong-looking.
#define A_CLEAR_R 32
#define A_CLEAR_G 96
#define A_CLEAR_B 32
#define B_CLEAR_R 96
#define B_CLEAR_G 32
#define B_CLEAR_B 96

static CGLContextObj ctx_a, ctx_b;
static GLuint fbo_a, fbo_b;

static void die(const char *m) { fprintf(stderr, "FATAL: %s\n", m); exit(1); }

// Side-effecting calls must never live inside assert(): this suite builds with
// -DNDEBUG (re-enabled by a later -UNDEBUG), and a flag-order change would
// silently delete the call along with the check.
#define MUST(expr, what) do { if (!(expr)) die(what); } while (0)
#define MUST_NOT(expr, what) do { if (expr) die(what); } while (0)

static void check_gl(const char *stage) {
    GLenum e = glGetError();
    if (e != GL_NO_ERROR) {
        fprintf(stderr, "FATAL: glGetError()=0x%04x after %s\n", e, stage);
        exit(1);
    }
}

static CGLPixelFormatObj choose_pixel_format(void) {
    CGLPixelFormatAttribute attrs[] = {
        kCGLPFAOpenGLProfile, (CGLPixelFormatAttribute)kCGLOGLPVersion_GL4_Core,
        kCGLPFAColorSize, (CGLPixelFormatAttribute)24,
        kCGLPFAAlphaSize, (CGLPixelFormatAttribute)8,
        kCGLPFAAccelerated,
        (CGLPixelFormatAttribute)0
    };
    CGLPixelFormatObj pf; GLint npix;
    if (CGLChoosePixelFormat(attrs, &pf, &npix) != kCGLNoError || !pf)
        die("CGLChoosePixelFormat");
    return pf;
}

// Framebuffer objects, like VAOs, are container objects and are NOT shared
// across contexts -- each context builds its own.
static GLuint make_framebuffer(int w, int h) {
    GLuint fbo, tex;
    glGenFramebuffers(1, &fbo);
    glBindFramebuffer(GL_FRAMEBUFFER, fbo);
    glGenTextures(1, &tex);
    glBindTexture(GL_TEXTURE_2D, tex);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, w, h, 0, GL_RGBA, GL_UNSIGNED_BYTE, NULL);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, tex, 0);
    if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE)
        die("FBO incomplete");
    glViewport(0, 0, w, h);
    return fbo;
}

static void use_a(void) {
    if (CGLSetCurrentContext(ctx_a) != kCGLNoError) die("CGLSetCurrentContext(A)");
    glBindFramebuffer(GL_FRAMEBUFFER, fbo_a);
    glViewport(0, 0, A_W, A_H);
}

static void use_b(void) {
    if (CGLSetCurrentContext(ctx_b) != kCGLNoError) die("CGLSetCurrentContext(B)");
    glBindFramebuffer(GL_FRAMEBUFFER, fbo_b);
    glViewport(0, 0, B_W, B_H);
}

// Draw the session and count GLYPH pixels, not merely changed ones.
//
// Counting "pixels different from the clear color" is far too weak: the
// terminal background fills ~96% of the framebuffer, so a session that drew
// its background but no text would score identically to a healthy one. Both
// child processes emit red text, and neither clear color nor the configured
// background (#102030) is red-dominant, so this metric goes to ~0 the moment
// glyph rendering breaks.
static long draw_and_count(kitty_session *s, int w, int h,
                           unsigned char cr, unsigned char cg, unsigned char cb,
                           const char *stage) {
    glClearColor(cr / 255.f, cg / 255.f, cb / 255.f, 1.f);
    glClear(GL_COLOR_BUFFER_BIT);
    bool drew = kitty_session_draw(s);
    if (!drew) { fprintf(stderr, "FATAL: draw failed at %s\n", stage); exit(1); }
    check_gl(stage);

    unsigned char *px = malloc((size_t)w * h * 4);
    assert(px);
    glReadPixels(0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, px);
    check_gl("glReadPixels");
    long filled = 0, glyph = 0;
    for (size_t i = 0; i < (size_t)w * h; i++) {
        unsigned char *p = px + i * 4;
        if (p[0] != cr || p[1] != cg || p[2] != cb) filled++;
        if (p[0] > 110 && p[0] > 2 * p[1] && p[0] > 2 * p[2]) glyph++;
    }
    unsigned char *c = px + ((size_t)(h/2) * w + (w/4)) * 4;
    printf("  %-36s glyph px: %5ld   (filled: %ld)  center=%02x%02x%02x%02x\n",
           stage, glyph, filled, c[0], c[1], c[2], c[3]);
    free(px);
    (void)filled;
    return glyph;
}

static void wait_for_output(kitty_session *s, const char *needle) {
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(s);
        char *text = kitty_session_text(s);
        bool saw = text && strstr(text, needle) != NULL;
        free(text);
        if (saw) return;
        usleep(10000);
    }
    fprintf(stderr, "FATAL: never saw %s\n", needle);
    exit(1);
}

int main(void) {
    CGLPixelFormatObj pf = choose_pixel_format();

    // The whole point: B is created sharing A's object space. An unshared
    // second context would fail to find the shader programs and font atlas
    // that render_init created in A.
    if (CGLCreateContext(pf, NULL, &ctx_a) != kCGLNoError) die("CGLCreateContext(A)");
    if (CGLCreateContext(pf, ctx_a, &ctx_b) != kCGLNoError)
        die("CGLCreateContext(B, shared with A)");
    CGLDestroyPixelFormat(pf);

    CGLSetCurrentContext(ctx_a);
    fbo_a = make_framebuffer(A_W, A_H);
    CGLSetCurrentContext(ctx_b);
    fbo_b = make_framebuffer(B_W, B_H);

    char err[512] = "";
    kitty_engine_config cfg = {
        .kitty_src_path = getenv("KITTY_SRC"),
        .libkitty_py_path = getenv("LIBKITTY_PY"),
        .config_path = getenv("LIBKITTY_TEST_CONFIG"),
    };
    kitty_engine *e = kitty_engine_init(&cfg, err, sizeof err);
    if (!e) { fprintf(stderr, "engine init failed: %s\n", err); return 1; }

    // Render init runs ONCE, in A. Shaders and the font atlas must reach B
    // through the share group; there is no second render_init anywhere below.
    use_a();
    int cw = 0, ch = 0;
    if (!kitty_render_init(e, 1.0, &cw, &ch, err, sizeof err)) {
        fprintf(stderr, "render init failed: %s\n", err);
        return 1;
    }
    assert(cw > 0 && ch > 0);
    check_gl("kitty_render_init");

    const char *mover_argv[] = {"/bin/sh", "-c",
        "printf '\\033[31mMOVER-SESSION\\033[m\\n'; sleep 60", NULL};
    const char *sentinel_argv[] = {"/bin/sh", "-c",
        "printf '\\033[31mSENTINEL-SESSION\\033[m\\n'; sleep 60", NULL};
    kitty_session *mover = kitty_session_create(e, 10, 60, mover_argv, NULL, err, sizeof err);
    if (!mover) { fprintf(stderr, "mover create failed: %s\n", err); return 1; }
    kitty_session *sentinel = kitty_session_create(e, 10, 60, sentinel_argv, NULL, err, sizeof err);
    if (!sentinel) { fprintf(stderr, "sentinel create failed: %s\n", err); return 1; }
    wait_for_output(mover, "MOVER-SESSION");
    wait_for_output(sentinel, "SENTINEL-SESSION");

    // --- 1. Both contexts render -------------------------------------------
    // Order matters: the mover's VAO is created in A first, then the
    // sentinel's in B, so the two are most likely to collide on GL name.
    printf("1. both contexts render\n");
    use_a();
    MUST(kitty_session_set_geometry(mover, A_W, A_H, 0, 0, A_W, A_H),
         "set_geometry(mover, A)");
    check_gl("set_geometry(mover, A)");
    long a0 = draw_and_count(mover, A_W, A_H, A_CLEAR_R, A_CLEAR_G, A_CLEAR_B,
                             "mover in A");
    assert(a0 > 200);   // glyphs, not background

    use_b();
    MUST(kitty_session_set_geometry(sentinel, B_W, B_H, 0, 0, B_W, B_H),
         "set_geometry(sentinel, B)");
    check_gl("set_geometry(sentinel, B)");
    long b0 = draw_and_count(sentinel, B_W, B_H, B_CLEAR_R, B_CLEAR_G, B_CLEAR_B,
                             "sentinel in B");
    assert(b0 > 200);

    // --- 2. Transfer A -> B, with a geometry change ------------------------
    printf("2. transfer A -> B\n");
    use_a();
    MUST(kitty_session_release_render_resources(mover), "release(mover) under A");
    check_gl("release(mover) under A");
    // Fails closed until geometry is reapplied.
    MUST_NOT(kitty_session_draw(mover), "draw must fail closed after release");

    use_b();
    // Half-width in B: proves the transfer survives new geometry rather than
    // reusing the stub's old A-sized viewport.
    MUST(kitty_session_set_geometry(mover, B_W, B_H, 0, 0, B_W / 2, B_H),
         "set_geometry(mover, B)");
    check_gl("set_geometry(mover, B)");
    long a1 = draw_and_count(mover, B_W, B_H, B_CLEAR_R, B_CLEAR_G, B_CLEAR_B,
                             "mover in B");
    assert(a1 > 200);
    long s1 = draw_and_count(sentinel, B_W, B_H, B_CLEAR_R, B_CLEAR_G, B_CLEAR_B,
                             "sentinel after mover release");
    assert(s1 > 200);   // releasing under A must not have touched B's VAOs

    // --- 3. Transfer B -> A, with another geometry change ------------------
    printf("3. transfer B -> A\n");
    use_b();
    MUST(kitty_session_release_render_resources(mover), "release(mover) under B");
    check_gl("release(mover) under B");
    long s2 = draw_and_count(sentinel, B_W, B_H, B_CLEAR_R, B_CLEAR_G, B_CLEAR_B,
                             "sentinel after 2nd release");
    assert(s2 > 200);

    use_a();
    MUST(kitty_session_set_geometry(mover, A_W, A_H, 0, 0, A_W, A_H),
         "set_geometry(mover, A again)");
    check_gl("set_geometry(mover, A again)");
    long a2 = draw_and_count(mover, A_W, A_H, A_CLEAR_R, A_CLEAR_G, A_CLEAR_B,
                             "mover back in A");
    assert(a2 > 200);

    // --- 4. Font-atlas rebuild while BOTH contexts hold live VAOs ----------
    // The mover's VAO lives in A and the sentinel's in B right now. A font
    // change drops every stub and rebuilds the shared atlas; both must recover.
    printf("4. font-atlas rebuild with live VAOs in both contexts\n");
    use_a();
    int ncw = 0, nch = 0;
    if (!kitty_render_set_font_size(e, 22.0, &ncw, &nch, err, sizeof err)) {
        fprintf(stderr, "font resize failed: %s\n", err);
        return 1;
    }
    check_gl("kitty_render_set_font_size");
    assert(ncw > 0 && nch > 0 && ncw != cw);

    MUST(kitty_session_set_geometry(mover, A_W, A_H, 0, 0, A_W, A_H),
         "set_geometry(mover, A after font rebuild)");
    long a3 = draw_and_count(mover, A_W, A_H, A_CLEAR_R, A_CLEAR_G, A_CLEAR_B,
                             "mover after font rebuild");
    assert(a3 > 200);

    use_b();
    MUST(kitty_session_set_geometry(sentinel, B_W, B_H, 0, 0, B_W, B_H),
         "set_geometry(sentinel, B after font rebuild)");
    long s3 = draw_and_count(sentinel, B_W, B_H, B_CLEAR_R, B_CLEAR_G, B_CLEAR_B,
                             "sentinel after font rebuild");
    assert(s3 > 200);

    // --- 5. Closing one context leaves the survivor rendering --------------
    // Release the sentinel under its OWN context before tearing that context
    // down; this is the window-close half of the lifecycle.
    printf("5. close context B, survivor keeps rendering\n");
    use_b();
    MUST(kitty_session_release_render_resources(sentinel),
         "release(sentinel) under B");
    check_gl("release(sentinel) under B");
    CGLSetCurrentContext(NULL);
    CGLDestroyContext(ctx_b);
    ctx_b = NULL;

    use_a();
    long a4 = draw_and_count(mover, A_W, A_H, A_CLEAR_R, A_CLEAR_G, A_CLEAR_B,
                             "mover after B destroyed");
    assert(a4 > 200);

    // The sentinel outlived its context: still a live session, and drawable
    // again once it is given geometry in the surviving context.
    MUST(kitty_session_set_geometry(sentinel, A_W, A_H, 0, 0, A_W, A_H),
         "set_geometry(sentinel, A after B destroyed)");
    check_gl("set_geometry(sentinel, A after B destroyed)");
    long s4 = draw_and_count(sentinel, A_W, A_H, A_CLEAR_R, A_CLEAR_G, A_CLEAR_B,
                             "sentinel rehomed into A");
    assert(s4 > 200);

    kitty_session_close(mover);
    kitty_session_close(sentinel);
    kitty_engine_shutdown(e);
    CGLSetCurrentContext(NULL);
    CGLDestroyContext(ctx_a);

    printf("MULTI-CONTEXT RENDER: OK (shared contexts, A->B->A transfer, "
           "geometry changes, font rebuild, sentinel intact, context teardown)\n");
    return 0;
}
