// Full-stack smoke test through the public API only: engine -> render init ->
// session running a real child -> pump -> set_viewport -> draw into a
// host-owned offscreen CGL framebuffer -> pixel assertions.
#define GL_SILENCE_DEPRECATION
#include "libkitty.h"
#include <OpenGL/OpenGL.h>
#include <OpenGL/gl3.h>
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define FB_W 800
#define FB_H 600
// Deliberately NOT terminal-black so untouched pixels are distinguishable
// from kitty's default background.
#define CLEAR_R 32
#define CLEAR_G 96
#define CLEAR_B 32

static void die(const char *m) { fprintf(stderr, "FATAL: %s\n", m); exit(1); }

static void make_gl_context(void) {
    CGLPixelFormatAttribute attrs[] = {
        kCGLPFAOpenGLProfile, (CGLPixelFormatAttribute)kCGLOGLPVersion_GL4_Core,
        kCGLPFAColorSize, (CGLPixelFormatAttribute)24,
        kCGLPFAAlphaSize, (CGLPixelFormatAttribute)8,
        kCGLPFAAccelerated,
        (CGLPixelFormatAttribute)0
    };
    CGLPixelFormatObj pf; GLint npix;
    if (CGLChoosePixelFormat(attrs, &pf, &npix) != kCGLNoError || !pf) die("CGLChoosePixelFormat");
    CGLContextObj ctx;
    if (CGLCreateContext(pf, NULL, &ctx) != kCGLNoError) die("CGLCreateContext");
    CGLDestroyPixelFormat(pf);
    CGLSetCurrentContext(ctx);
}

static void make_framebuffer(void) {
    GLuint fbo, tex;
    glGenFramebuffers(1, &fbo);
    glBindFramebuffer(GL_FRAMEBUFFER, fbo);
    glGenTextures(1, &tex);
    glBindTexture(GL_TEXTURE_2D, tex);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, FB_W, FB_H, 0, GL_RGBA, GL_UNSIGNED_BYTE, NULL);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, tex, 0);
    if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE) die("FBO incomplete");
    glViewport(0, 0, FB_W, FB_H);
}

static unsigned char *capture_focus_frame(kitty_session *s,
                                          bool pane_active,
                                          bool window_focused,
                                          bool single_pane) {
    glClearColor(CLEAR_R / 255.f, CLEAR_G / 255.f, CLEAR_B / 255.f, 1.f);
    glClear(GL_COLOR_BUFFER_BIT);
    assert(kitty_session_draw_with_state(
        s, false, pane_active, window_focused, single_pane));
    unsigned char *pixels = malloc((size_t)FB_W * FB_H * 4);
    assert(pixels);
    glReadPixels(0, 0, FB_W, FB_H, GL_RGBA, GL_UNSIGNED_BYTE, pixels);
    return pixels;
}

static long differing_pixels(const unsigned char *a, const unsigned char *b,
                             unsigned long long *byte_delta) {
    long changed = 0;
    unsigned long long delta = 0;
    for (size_t i = 0; i < (size_t)FB_W * FB_H; i++) {
        const unsigned char *pa = a + i * 4, *pb = b + i * 4;
        bool pixel_changed = false;
        for (int channel = 0; channel < 4; channel++) {
            int d = (int)pa[channel] - (int)pb[channel];
            if (d) pixel_changed = true;
            delta += (unsigned long long)(d < 0 ? -d : d);
        }
        if (pixel_changed) changed++;
    }
    if (byte_delta) *byte_delta = delta;
    return changed;
}

static void write_focus_config(FILE *f, const char *inactive_text_alpha) {
    fprintf(f,
        "font_size 17.0\n"
        "foreground #a1b2c3\n"
        "background #102030\n"
        "background_opacity 0.65\n"
        "selection_foreground #050607\n"
        "selection_background #d0a0f0\n"
        "inactive_text_alpha %s\n",
        inactive_text_alpha);
}

int main(void) {
    make_gl_context();
    make_framebuffer();
    glClearColor(CLEAR_R / 255.f, CLEAR_G / 255.f, CLEAR_B / 255.f, 1.f);
    glClear(GL_COLOR_BUFFER_BIT);

    char err[512] = "";
    kitty_engine_config cfg = {
        .kitty_src_path = getenv("KITTY_SRC"),
        .libkitty_py_path = getenv("LIBKITTY_PY"),
        .config_path = getenv("LIBKITTY_TEST_CONFIG"),
    };
    kitty_engine *e = kitty_engine_init(&cfg, err, sizeof err);
    if (!e) { fprintf(stderr, "engine init failed: %s\n", err); return 1; }

    int cw = 0, ch = 0;
    if (!kitty_render_init(e, 1.0, &cw, &ch, err, sizeof err)) {
        fprintf(stderr, "render init failed: %s\n", err);
        return 1;
    }
    assert(cw > 0 && ch > 0);

    const char *argv[] = {"/bin/sh", "-c",
        "printf '\\033[31mRENDER-OLD\\033[m\\nRENDER-NEW\\n'; sleep 30", NULL};
    kitty_session *s = kitty_session_create(e, 10, 60, argv, NULL, err, sizeof err);
    if (!s) { fprintf(stderr, "session create failed: %s\n", err); return 1; }

    int saw_output = 0;
    for (int i = 0; i < 300; i++) {
        kitty_session_pump(s);
        char *text = kitty_session_text(s);
        saw_output = text && strstr(text, "RENDER-NEW") != NULL;
        free(text);
        if (saw_output) break;
        usleep(10000);
    }
    assert(saw_output);

    // Keep the calls outside assert: with NDEBUG they would vanish entirely.
    bool viewport_ok = kitty_session_set_viewport(s, FB_W, FB_H);
    assert(viewport_ok);
    bool draw_ok = kitty_session_draw(s);
    assert(draw_ok);

    unsigned char *px = malloc((size_t)FB_W * FB_H * 4);
    glReadPixels(0, 0, FB_W, FB_H, GL_RGBA, GL_UNSIGNED_BYTE, px);
    long changed = 0, reddish = 0, translucent = 0;
    unsigned char minimum_alpha = 255, maximum_alpha = 0;
    for (size_t i = 0; i < (size_t)FB_W * FB_H; i++) {
        unsigned char *p = px + i * 4;
        if (p[0] != CLEAR_R || p[1] != CLEAR_G || p[2] != CLEAR_B) changed++;
        if (p[0] > 120 && p[0] > 2 * p[1] && p[0] > 2 * p[2]) reddish++;
        if (p[3] < minimum_alpha) minimum_alpha = p[3];
        if (p[3] > maximum_alpha) maximum_alpha = p[3];
        if (p[3] > 0 && p[3] < 255) translucent++;
    }
    printf("pixels changed: %ld, red-dominant: %ld, translucent: %ld, alpha: %u..%u\n",
           changed, reddish, translucent, minimum_alpha, maximum_alpha);
    assert(changed > 1000 && reddish > 50);
    // F3: the fixture's 0.65 opacity reaches kitty's default-background cell
    // shader, while text/cursor/special cells can still reach full opacity.
    assert(minimum_alpha >= 160 && minimum_alpha <= 170);
    assert(maximum_alpha == 255);
    assert(translucent > 1000);

    // F7: Kitmux's negative default fades only a nonfocused pane in a split.
    // It deliberately ignores the host window's key state, so the focused
    // pane remains full-strength even when the app itself is inactive.
    unsigned char *focused_default = capture_focus_frame(s, true, true, false);
    unsigned char *inactive_default = capture_focus_frame(s, false, true, false);
    unsigned char *unfocused_window_default = capture_focus_frame(s, true, false, false);
    unsigned long long default_delta = 0;
    long default_changed = differing_pixels(
        focused_default, inactive_default, &default_delta);
    assert(default_changed > 50 && default_delta > 0);
    assert(memcmp(focused_default, unfocused_window_default,
                  (size_t)FB_W * FB_H * 4) == 0);
    free(inactive_default);
    free(unfocused_window_default);

    // An explicit positive kitty.conf value keeps upstream Kitty semantics:
    // it is stronger here and also fades a single pane when the OS window is
    // not focused.
    char focus_conf[] = "/tmp/libkitty-render-focus-XXXXXX";
    int focus_fd = mkstemp(focus_conf);
    assert(focus_fd >= 0);
    close(focus_fd);
    FILE *focus_f = fopen(focus_conf, "w");
    assert(focus_f);
    write_focus_config(focus_f, "0.35");
    fclose(focus_f);
    assert(kitty_engine_reload_config(e, focus_conf, err, sizeof err) == 1);

    unsigned char *focused_override = capture_focus_frame(s, true, true, false);
    unsigned char *inactive_override = capture_focus_frame(s, false, true, false);
    unsigned long long override_delta = 0;
    long override_changed = differing_pixels(
        focused_override, inactive_override, &override_delta);
    assert(override_changed > 50 && override_delta > default_delta * 2);
    unsigned char *focused_single = capture_focus_frame(s, true, true, true);
    unsigned char *unfocused_single = capture_focus_frame(s, true, false, true);
    assert(differing_pixels(focused_single, unfocused_single, NULL) > 50);
    free(focused_override); free(inactive_override);
    free(focused_single); free(unfocused_single);

    // Setting 1.0 explicitly disables the visual fade while still exercising
    // the same focus-state render path.
    focus_f = fopen(focus_conf, "w");
    assert(focus_f);
    write_focus_config(focus_f, "1.0");
    fclose(focus_f);
    assert(kitty_engine_reload_config(e, focus_conf, err, sizeof err) == 1);
    unsigned char *focused_disabled = capture_focus_frame(s, true, true, false);
    unsigned char *inactive_disabled = capture_focus_frame(s, false, true, false);
    assert(memcmp(focused_disabled, inactive_disabled,
                  (size_t)FB_W * FB_H * 4) == 0);
    free(focused_disabled); free(inactive_disabled);
    free(focused_default);

    assert(kitty_engine_reload_config(
        e, getenv("LIBKITTY_TEST_CONFIG"), err, sizeof err) == 1);
    unlink(focus_conf);
    glClear(GL_COLOR_BUFFER_BIT);
    assert(kitty_session_draw_with_state(s, true, true, true, true));
    printf("focus alpha: default changed=%ld delta=%llu, override changed=%ld delta=%llu\n",
           default_changed, default_delta, override_changed, override_delta);

    // Backlog 05: the additive host blink phase hides only the cursor for one
    // frame and restores the terminal's own visibility mode immediately.
    draw_ok = kitty_session_draw_with_cursor_visibility(s, false);
    assert(draw_ok);
    unsigned char *cursor_hidden_px = malloc((size_t)FB_W * FB_H * 4);
    glReadPixels(0, 0, FB_W, FB_H, GL_RGBA, GL_UNSIGNED_BYTE, cursor_hidden_px);
    long cursor_changed = 0;
    for (size_t i = 0; i < (size_t)FB_W * FB_H; i++) {
        unsigned char *before = px + i * 4;
        unsigned char *after = cursor_hidden_px + i * 4;
        if (before[0] != after[0] || before[1] != after[1] ||
            before[2] != after[2] || before[3] != after[3]) cursor_changed++;
    }
    free(cursor_hidden_px);
    printf("cursor-phase changed pixels: %ld\n", cursor_changed);
    assert(cursor_changed > 0);
    draw_ok = kitty_session_draw_with_cursor_visibility(s, true);
    assert(draw_ok);

    // A terminal-requested steady cursor (DECSCUSR 2) is not eligible for
    // host blinking, so the hidden host phase must leave its pixels intact.
    const char *steady_argv[] = {"/bin/sh", "-c",
        "printf '\033[2 qSTEADY'; sleep 30", NULL};
    kitty_session *steady = kitty_session_create(
        e, 10, 60, steady_argv, NULL, err, sizeof err);
    assert(steady);
    for (int i = 0; i < 100; i++) {
        kitty_session_pump(steady);
        usleep(10000);
    }
    assert(kitty_session_set_viewport(steady, FB_W, FB_H));
    assert(kitty_session_draw_with_cursor_visibility(steady, true));
    unsigned char *steady_visible = malloc((size_t)FB_W * FB_H * 4);
    unsigned char *steady_hidden = malloc((size_t)FB_W * FB_H * 4);
    glReadPixels(0, 0, FB_W, FB_H, GL_RGBA, GL_UNSIGNED_BYTE, steady_visible);
    assert(kitty_session_draw_with_cursor_visibility(steady, false));
    glReadPixels(0, 0, FB_W, FB_H, GL_RGBA, GL_UNSIGNED_BYTE, steady_hidden);
    long steady_changed = 0;
    for (size_t i = 0; i < (size_t)FB_W * FB_H * 4; i++) {
        if (steady_visible[i] != steady_hidden[i]) steady_changed++;
    }
    free(steady_visible); free(steady_hidden);
    kitty_session_close(steady);
    printf("steady-cursor changed bytes: %ld\n", steady_changed);
    assert(steady_changed == 0);

    // Search uses kitty's native mark attributes/colors in the same render
    // path. The current line is mark 2 while the other result remains mark 1.
    size_t search_matches = kitty_session_search_set(s, "render", 6);
    assert(search_matches == 2);
    assert(kitty_session_search_visible_mark_count(s, 1) > 0);
    assert(kitty_session_search_visible_mark_count(s, 2) > 0);
    draw_ok = kitty_session_draw(s);
    assert(draw_ok);
    unsigned char *searched_px = malloc((size_t)FB_W * FB_H * 4);
    glReadPixels(0, 0, FB_W, FB_H, GL_RGBA, GL_UNSIGNED_BYTE, searched_px);
    long search_changed = 0;
    for (size_t i = 0; i < (size_t)FB_W * FB_H; i++) {
        unsigned char *before = px + i * 4;
        unsigned char *after = searched_px + i * 4;
        if (before[0] != after[0] || before[1] != after[1] ||
            before[2] != after[2]) search_changed++;
    }
    free(searched_px);
    printf("search-changed pixels: %ld\n", search_changed);
    assert(search_changed > 100);
    kitty_session_search_clear(s);

    // v0.5 selection uses kitty's native selection mask in the same render
    // path. Selecting the first RENDER line must visibly change the pixels.
    kitty_session_selection_start(s, 0, 0, true, 0);
    kitty_session_selection_update(s, 8, 0, false, true);
    draw_ok = kitty_session_draw(s);
    assert(draw_ok);
    unsigned char *selected_px = malloc((size_t)FB_W * FB_H * 4);
    glReadPixels(0, 0, FB_W, FB_H, GL_RGBA, GL_UNSIGNED_BYTE, selected_px);
    long selection_changed = 0;
    for (size_t i = 0; i < (size_t)FB_W * FB_H; i++) {
        unsigned char *before = px + i * 4;
        unsigned char *after = selected_px + i * 4;
        if (before[0] != after[0] || before[1] != after[1] ||
            before[2] != after[2]) selection_changed++;
    }
    free(selected_px);
    free(px);
    printf("selection-changed pixels: %ld\n", selection_changed);
    assert(selection_changed > 100);

    // ---- v0.2: two sessions composited into one framebuffer ----
    const char *argv2[] = {"/bin/sh", "-c",
        "printf '\\033[34mBLUE-PANE\\033[m\\n'; sleep 30", NULL};
    kitty_session *s2 = kitty_session_create(e, 10, 40, argv2, NULL, err, sizeof err);
    if (!s2) { fprintf(stderr, "session2 create failed: %s\n", err); return 1; }
    int saw2 = 0;
    for (int i = 0; i < 300 && !saw2; i++) {
        kitty_session_pump(s2);
        char *t = kitty_session_text(s2);
        saw2 = t && strstr(t, "BLUE-PANE") != NULL;
        free(t);
        if (!saw2) usleep(10000);
    }
    assert(saw2);

    glClear(GL_COLOR_BUFFER_BIT);
    bool g1 = kitty_session_set_geometry(s, FB_W, FB_H, 0, 0, 390, FB_H);
    bool g2 = kitty_session_set_geometry(s2, FB_W, FB_H, 410, 0, FB_W, FB_H);
    assert(g1 && g2);
    bool d1 = kitty_session_draw(s);
    bool d2 = kitty_session_draw(s2);
    assert(d1 && d2);

    px = malloc((size_t)FB_W * FB_H * 4);
    glReadPixels(0, 0, FB_W, FB_H, GL_RGBA, GL_UNSIGNED_BYTE, px);
    long left_painted = 0, right_painted = 0, gap_painted = 0;
    for (int y = 0; y < FB_H; y++) {
        for (int x = 0; x < FB_W; x++) {
            unsigned char *p = px + ((size_t)y * FB_W + x) * 4;
            int painted = p[0] != CLEAR_R || p[1] != CLEAR_G || p[2] != CLEAR_B;
            if (!painted) continue;
            if (x < 390) left_painted++;
            else if (x >= 410) right_painted++;
            else gap_painted++;
        }
    }
    free(px);
    printf("composite: left=%ld right=%ld gap=%ld\n", left_painted, right_painted, gap_painted);
    assert(left_painted > 1000 && right_painted > 1000);
    assert(gap_painted == 0);   // the 20px gap column stays clear color

    // v0.15 / S4 spike gate: a config reload rethemes already-rendered
    // sessions through the real draw path. The new config uses a loud opaque
    // red background; after reload + redraw, both panes' default-background
    // cells must show it, and none may keep the old fixture background.
    char reload_conf[] = "/tmp/libkitty-render-reload-XXXXXX";
    int reload_fd = mkstemp(reload_conf);
    assert(reload_fd >= 0);
    close(reload_fd);
    FILE *reload_f = fopen(reload_conf, "w");
    assert(reload_f);
    fputs("background #ff2030\nbackground_opacity 1.0\n", reload_f);
    fclose(reload_f);

    // Red-dominant + fully opaque marks the new background. This offscreen
    // FBO is not sRGB, so kitty's shader writes linear values — #ff2030
    // arrives as roughly (255,4,8) — which exact-match comparisons would
    // miss; dominance is color-space-agnostic. Before the reload only the
    // sparse red "RENDER-OLD" glyphs qualify.
    px = malloc((size_t)FB_W * FB_H * 4);
    glReadPixels(0, 0, FB_W, FB_H, GL_RGBA, GL_UNSIGNED_BYTE, px);
    long new_bg_before = 0;
    for (size_t i = 0; i < (size_t)FB_W * FB_H; i++) {
        unsigned char *p = px + i * 4;
        if (p[0] > 200 && p[1] < 20 && p[2] < 20 && p[3] == 255) new_bg_before++;
    }
    free(px);
    assert(new_bg_before < 5000);  // no red *background* yet, glyphs only

    int reload_patched = kitty_engine_reload_config(
        e, reload_conf, err, sizeof err);
    if (reload_patched < 0) fprintf(stderr, "reload failed: %s\n", err);
    assert(reload_patched == 2);  // both live sessions patched

    glClear(GL_COLOR_BUFFER_BIT);
    d1 = kitty_session_draw(s);
    d2 = kitty_session_draw(s2);
    assert(d1 && d2);
    px = malloc((size_t)FB_W * FB_H * 4);
    glReadPixels(0, 0, FB_W, FB_H, GL_RGBA, GL_UNSIGNED_BYTE, px);
    long new_bg_left = 0, new_bg_right = 0, old_bg = 0;
    for (int y = 0; y < FB_H; y++) {
        for (int x = 0; x < FB_W; x++) {
            unsigned char *p = px + ((size_t)y * FB_W + x) * 4;
            if (p[0] > 200 && p[1] < 20 && p[2] < 20 && p[3] == 255) {
                if (x < 390) new_bg_left++;
                else if (x >= 410) new_bg_right++;
            }
            // The fixture rendered default-background cells at alpha ~166
            // (0.65 opacity); any survivor means a stale stub opacity.
            if (p[3] > 160 && p[3] < 170) old_bg++;
        }
    }
    free(px);
    printf("reload retheme: left=%ld right=%ld stale=%ld\n",
           new_bg_left, new_bg_right, old_bg);
    assert(new_bg_left > 100000 && new_bg_right > 100000);
    assert(old_bg == 0);

    // Restore the fixture config so the font assertions below keep their
    // original environment.
    assert(kitty_engine_reload_config(
        e, getenv("LIBKITTY_TEST_CONFIG"), err, sizeof err) == 2);
    unlink(reload_conf);
    printf("reload retheme OK\n");

    // v0.8 app-wide font reconfiguration keeps live sessions and their
    // contents, replaces shared cell metrics, and lets geometry resize both
    // PTYs against the new grid.
    double default_font_size = kitty_render_font_size(e);
    assert(default_font_size >= 4.0);
    int original_cw = cw, original_ch = ch;
    bool invalid_font = kitty_render_set_font_size(
        e, 3.0, &cw, &ch, err, sizeof err);
    assert(!invalid_font);
    assert(kitty_render_font_size(e) == default_font_size);
    assert(cw == original_cw && ch == original_ch);

    bool font_ok = kitty_render_set_font_size(
        e, default_font_size + 4.0, &cw, &ch, err, sizeof err);
    assert(font_ok);
    assert(kitty_render_font_size(e) == default_font_size + 4.0);
    assert(cw >= original_cw && ch > original_ch);
    g1 = kitty_session_set_geometry(s, FB_W, FB_H, 0, 0, 390, FB_H);
    g2 = kitty_session_set_geometry(s2, FB_W, FB_H, 410, 0, FB_W, FB_H);
    assert(g1 && g2);
    d1 = kitty_session_draw(s);
    d2 = kitty_session_draw(s2);
    assert(d1 && d2);
    char *font_text1 = kitty_session_text(s);
    char *font_text2 = kitty_session_text(s2);
    assert(font_text1 && strstr(font_text1, "RENDER-OLD"));
    assert(font_text2 && strstr(font_text2, "BLUE-PANE"));
    free(font_text1);
    free(font_text2);

    font_ok = kitty_render_set_font_size(
        e, default_font_size, &cw, &ch, err, sizeof err);
    assert(font_ok);
    assert(cw == original_cw && ch == original_ch);

    kitty_session_close(s2);
    kitty_session_close(s);
    kitty_engine_shutdown(e);
    printf("render smoke OK\n");
    return 0;
}
