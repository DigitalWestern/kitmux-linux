#include "libkitty.h"
#include <assert.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

static int damage_count = 0, exited = 0, exit_status = -1;
static char last_title[256];
static void on_damage(void *u) { (void)u; damage_count++; }
static void on_title(void *u, const char *t) { (void)u; snprintf(last_title, sizeof last_title, "%s", t); }
static void on_exit_cb(void *u, int status) { (void)u; exited = 1; exit_status = status; }

// v0.9 notifications: record every callback as "title|body" in arrival order.
#define MAX_NOTES 16
static int note_count = 0;
static char note_log[MAX_NOTES][4096];
static void on_note(void *u, const char *title, const char *body) {
    (void)u;
    if (note_count < MAX_NOTES)
        snprintf(note_log[note_count], sizeof note_log[0], "%s|%s", title, body);
    note_count++;
}

// v0.11 user vars: record every OSC 1337 SetUserVar callback as "key|value".
static int uvar_count = 0;
static char uvar_log[MAX_NOTES][4096];
static void on_uvar(void *u, const char *key, const char *value) {
    (void)u;
    if (uvar_count < MAX_NOTES)
        snprintf(uvar_log[uvar_count], sizeof uvar_log[0], "%s|%s", key, value);
    uvar_count++;
}

static void write_file(const char *path, const char *content) {
    FILE *f = fopen(path, "w");
    assert(f);
    fputs(content, f);
    fclose(f);
}

static void pump_for(kitty_session *s, int iterations) {
    for (int i = 0; i < iterations; i++) {
        kitty_session_pump(s);
        usleep(10000);
    }
}

static bool wait_for_foreground_state(kitty_session *s, bool expected) {
    for (int i = 0; i < 300; i++) {
        kitty_session_pump(s);
        if (kitty_session_has_foreground_process(s) == expected) return true;
        usleep(10000);
    }
    return false;
}

static void test_visible_line_wrap_metadata(kitty_engine *e,
                                            char *err, size_t errlen) {
    const char *argv[] = {
        "/bin/sh", "-c", "printf AAAAAAAAAAAAAAAAAAAAA; sleep 1", NULL
    };
    kitty_session *s = kitty_session_create(e, 4, 10, argv, NULL, err, errlen);
    assert(s);
    pump_for(s, 30);
    uint8_t wraps[4] = {0};
    assert(kitty_session_line_wraps(s, wraps, sizeof wraps) == 4);
    assert(wraps[0] == 1 && wraps[1] == 1 && wraps[2] == 0 && wraps[3] == 0);
    assert(kitty_session_line_wraps(NULL, wraps, sizeof wraps) == 0);
    assert(kitty_session_line_wraps(s, NULL, sizeof wraps) == 0);
    assert(kitty_session_line_wraps(s, wraps, 0) == 0);
    kitty_session_close(s);
}

static void test_environment_overrides(kitty_engine *e,
                                       char *err, size_t errlen) {
    const char *argv[] = {
        "/bin/sh", "-c", "printf 'ENV=%s' \"$KITMUX_PANE_ID\"; sleep 1", NULL
    };
    const char *env[] = {"KITMUX_PANE_ID=pane-test-123", NULL};
    kitty_session *s = kitty_session_create_with_options(
        e, 4, 60, argv, NULL, env, NULL, err, errlen);
    assert(s);
    pump_for(s, 30);
    char *text = kitty_session_text(s);
    assert(text && strstr(text, "ENV=pane-test-123"));
    free(text);
    kitty_session_close(s);
    printf("environment overrides OK\n");
}

// v0.17 process-aware close gate. The stopped-job decision is intentionally
// true: a suspended editor still owns user state. A running background job is
// false because the interactive shell owns the terminal again.
static void test_foreground_process_signal(kitty_engine *e, char *err, size_t errlen) {
    const char *shell_argv[] = {"/bin/zsh", "-f", NULL};
    kitty_session *s = kitty_session_create(e, 8, 80, shell_argv, NULL, err, errlen);
    assert(s);
    pump_for(s, 20);
    assert(!kitty_session_has_foreground_process(NULL));
    assert(!kitty_session_has_foreground_process(s));       // idle shell

    const uint8_t foreground[] = "sleep 30\n";
    kitty_session_write(s, foreground, sizeof foreground - 1);
    assert(wait_for_foreground_state(s, true));              // foreground job

    const uint8_t ctrl_z = 0x1a;
    kitty_session_write(s, &ctrl_z, 1);
    pump_for(s, 30);
    assert(kitty_session_has_foreground_process(s));         // stopped job

    const uint8_t background[] = "bg\n";
    kitty_session_write(s, background, sizeof background - 1);
    assert(wait_for_foreground_state(s, false));             // running background job
    kitty_session_close(s);

    const char *exit_argv[] = {"/bin/sh", "-c", "exit 0", NULL};
    s = kitty_session_create(e, 4, 40, exit_argv, NULL, err, errlen);
    assert(s);
    for (int i = 0; i < 300 && kitty_session_child_alive(s); i++) {
        kitty_session_pump(s);
        usleep(10000);
    }
    assert(!kitty_session_child_alive(s));
    assert(!kitty_session_has_foreground_process(s));       // exited child
    kitty_session_close(s);
    printf("foreground process signal OK\n");
}

// v0.15 config reload: new options replace the old ones globally and patch
// every live session's color profile; kitmux defaults re-apply to unset keys;
// any failure leaves the last good configuration fully active.
static void test_config_reload(kitty_engine *e) {
    char err[512] = "";
    double v = 0;

    // Baseline: fixture values are active.
    assert(kitty_engine_config_number(e, "background", &v) && v == (double)0x102030);
    assert(kitty_engine_config_number(e, "cursor", &v) && v == (double)0xFFFFFF);
    assert(kitty_engine_config_number(e, "inactive_text_alpha", &v) && v == -0.85);

    char path[] = "/tmp/libkitty-reload-XXXXXX";
    int fd = mkstemp(path);
    assert(fd >= 0);
    close(fd);
    write_file(path,
        "background #405060\nforeground #fedcba\ninactive_text_alpha 0.72\n");
    int patched = kitty_engine_reload_config(e, path, err, sizeof err);
    if (patched < 0) fprintf(stderr, "reload failed: %s\n", err);
    assert(patched == 1);  // exactly one live session at this call site
    assert(kitty_engine_config_number(e, "background", &v) && v == (double)0x405060);
    assert(kitty_engine_config_number(e, "foreground", &v) && v == (double)0xFEDCBA);
    assert(kitty_engine_config_number(e, "cursor", &v) && v == (double)0xFFFFFF);
    assert(kitty_engine_config_number(e, "inactive_text_alpha", &v) && v == 0.72);

    // A missing file fails with a message and leaves the last good config.
    err[0] = 0;
    assert(kitty_engine_reload_config(e, "/nonexistent/kitty.conf", err, sizeof err) == -1);
    assert(err[0] != 0);
    assert(kitty_engine_config_number(e, "background", &v) && v == (double)0x405060);

    // An unreadable file must fail too — kitty's loader would silently skip
    // it and hand back pure defaults, losing the user's configuration.
    err[0] = 0;
    assert(chmod(path, 0) == 0);
    assert(kitty_engine_reload_config(e, path, err, sizeof err) == -1);
    assert(err[0] != 0);
    assert(kitty_engine_config_number(e, "background", &v) && v == (double)0x405060);
    assert(chmod(path, 0644) == 0);

    // Restore the fixture for every assertion that follows.
    assert(kitty_engine_reload_config(e, getenv("LIBKITTY_TEST_CONFIG"), err, sizeof err) >= 0);
    assert(kitty_engine_config_number(e, "background", &v) && v == (double)0x102030);
    unlink(path);
    printf("config reload OK\n");
}

int main(void) {
    char err[512] = "";
    kitty_engine_config cfg = {
        .kitty_src_path = getenv("KITTY_SRC"),
        .libkitty_py_path = getenv("LIBKITTY_PY"),
        .config_path = getenv("LIBKITTY_TEST_CONFIG"),
    };
    kitty_engine *e = kitty_engine_init(&cfg, err, sizeof err);
    if (!e) { fprintf(stderr, "init failed: %s\n", err); return 1; }

    const char *argv[] = {"/bin/sh", "-c",
        "printf '\\033]2;marker-title\\007'; echo session-marker; exit 7", NULL};
    kitty_session_callbacks cbs = {
        .on_damage = on_damage, .on_title = on_title, .on_child_exit = on_exit_cb,
    };
    kitty_session *s = kitty_session_create(e, 10, 60, argv, &cbs, err, sizeof err);
    if (!s) { fprintf(stderr, "session create failed: %s\n", err); return 1; }
    assert(kitty_session_fd(s) > 0);

    for (int i = 0; i < 400 && !exited; i++) { kitty_session_pump(s); usleep(10000); }

    char *text = kitty_session_text(s);
    assert(text);
    assert(strstr(text, "session-marker"));
    assert(damage_count > 0);
    assert(strcmp(last_title, "marker-title") == 0);
    assert(exited && exit_status == 7);
    assert(!kitty_session_child_alive(s));
    free(text);

    // v0.1 key encoding: legacy mode (cursor-key mode off, no kitty flags)
    char keybuf[64];
    kitty_key_event up = { .key = 0xe008 /* GLFW_FKEY_UP */, .action = 1 };
    size_t n = kitty_session_encode_key(s, &up, keybuf, sizeof keybuf);
    assert(n == 3 && memcmp(keybuf, "\x1b[A", 3) == 0);
    kitty_key_event ctrl_c = { .key = 'c', .mods = 0x4, .action = 1 };
    n = kitty_session_encode_key(s, &ctrl_c, keybuf, sizeof keybuf);
    assert(n == 1 && keybuf[0] == 0x03);
    // v0.13 generic config access: the fixture proves colors and ordinary
    // numeric options came through kitty's real parser.
    assert(kitty_engine_option_as_alt(e) == 3);
    double config_value = -1;
    assert(kitty_engine_config_number(e, "background", &config_value));
    assert(config_value == 0x102030);
    assert(kitty_engine_config_number(e, "foreground", &config_value));
    assert(config_value == 0xa1b2c3);
    assert(kitty_engine_config_number(e, "font_size", &config_value));
    assert(config_value == 17.0);
    assert(kitty_engine_config_number(e, "background_opacity", &config_value));
    assert(config_value == 0.65);
    assert(kitty_engine_config_number(e, "inactive_text_alpha", &config_value));
    assert(config_value == -0.85);
    assert(!kitty_engine_config_number(e, "not_a_real_option", &config_value));
    // F7 tuple indexing: "key:index" reaches into tuple options.
    double decay_fast = -1, decay_slow = -1;
    assert(kitty_engine_config_number(e, "cursor_trail_decay:0", &decay_fast));
    assert(kitty_engine_config_number(e, "cursor_trail_decay:1", &decay_slow));
    assert(decay_fast > 0 && decay_slow >= decay_fast);
    assert(!kitty_engine_config_number(e, "cursor_trail_decay:9", &config_value));
    assert(!kitty_engine_config_number(e, "cursor_trail_decay:x", &config_value));
    assert(!kitty_engine_config_number(e, "font_size:0", &config_value));
    assert(!kitty_engine_config_number(NULL, "background", &config_value));
    assert(!kitty_engine_config_number(e, "background", NULL));
    // v0.16: string options keep their parsed value and ownership in Kitty.
    char *bell_marker = kitty_engine_config_string(e, "bell_on_tab");
    assert(bell_marker && strcmp(bell_marker, "ALERT ") == 0);
    free(bell_marker);
    assert(!kitty_engine_config_string(e, "font_size"));
    assert(!kitty_engine_config_string(e, "not_a_real_option"));
    assert(!kitty_engine_config_string(NULL, "bell_on_tab"));
    assert(!kitty_engine_config_string(e, NULL));
    test_config_reload(e);  // v0.15: exactly one live session (s) here
    kitty_session_close(s);
    test_environment_overrides(e, err, sizeof err);
    test_foreground_process_signal(e, err, sizeof err);

    // v0.6 cwd: kitty's OSC 7 parser + URL decoder, session isolation,
    // create-with-cwd, missing-directory fallback, and cwd-only zsh
    // integration.
    const char *osc_argv[] = {"/bin/sh", "-c",
        "printf '\\033]7;file://localhost/private/tmp/a%%20b\\007'; "
        "printf CWDREADY; sleep 30", NULL};
    kitty_session *cs = kitty_session_create(
        e, 5, 60, osc_argv, NULL, err, sizeof err);
    assert(cs);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(cs);
        char *t = kitty_session_text(cs);
        int ready = t && strstr(t, "CWDREADY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    char *reported_cwd = kitty_session_reported_cwd(cs);
    assert(reported_cwd && strcmp(reported_cwd, "/private/tmp/a b") == 0);
    free(reported_cwd);

    const char *quiet_argv[] = {"/bin/sh", "-c", "printf QUIET; sleep 30", NULL};
    kitty_session *quiet = kitty_session_create(
        e, 5, 60, quiet_argv, NULL, err, sizeof err);
    assert(quiet);
    reported_cwd = kitty_session_reported_cwd(quiet);
    assert(reported_cwd && strcmp(reported_cwd, "") == 0);
    free(reported_cwd);
    kitty_session_close(quiet);
    kitty_session_close(cs);

    // F7 cursor cell: after printing "abc" the cursor sits at column 3 of
    // row 0 and is visible; a DECTCEM hide (CSI ?25l) flips visible off.
    const char *cursor_argv[] = {"/bin/sh", "-c",
        "printf abc; sleep 30", NULL};
    kitty_session *cursor_s = kitty_session_create(
        e, 5, 60, cursor_argv, NULL, err, sizeof err);
    assert(cursor_s);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(cursor_s);
        char *t = kitty_session_text(cursor_s);
        int ready = t && strstr(t, "abc");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    int cur_x = -1, cur_y = -1;
    bool cur_visible = false;
    assert(kitty_session_cursor_cell(cursor_s, &cur_x, &cur_y, &cur_visible));
    assert(cur_x == 3 && cur_y == 0 && cur_visible);
    assert(!kitty_session_cursor_cell(NULL, &cur_x, &cur_y, &cur_visible));
    kitty_session_close(cursor_s);

    const char *hidden_argv[] = {"/bin/sh", "-c",
        "printf '\\033[?25l'; printf HIDDEN; sleep 30", NULL};
    kitty_session *hidden_s = kitty_session_create(
        e, 5, 60, hidden_argv, NULL, err, sizeof err);
    assert(hidden_s);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(hidden_s);
        char *t = kitty_session_text(hidden_s);
        int ready = t && strstr(t, "HIDDEN");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    assert(kitty_session_cursor_cell(hidden_s, &cur_x, &cur_y, &cur_visible));
    assert(cur_x == 6 && cur_y == 0 && !cur_visible);
    kitty_session_close(hidden_s);

    const char *kitty_osc_argv[] = {"/bin/sh", "-c",
        "printf '\\033]7;kitty-shell-cwd://host/Users/test/project\\007'; "
        "sleep 30", NULL};
    kitty_session *kitty_osc = kitty_session_create(
        e, 5, 60, kitty_osc_argv, NULL, err, sizeof err);
    assert(kitty_osc);
    int saw_kitty_osc = 0;
    for (int i = 0; i < 400 && !saw_kitty_osc; i++) {
        kitty_session_pump(kitty_osc);
        reported_cwd = kitty_session_reported_cwd(kitty_osc);
        saw_kitty_osc = reported_cwd && reported_cwd[0];
        if (saw_kitty_osc) {
            assert(strcmp(reported_cwd, "/Users/test/project") == 0);
            free(reported_cwd);
            break;
        }
        free(reported_cwd);
        usleep(10000);
    }
    assert(saw_kitty_osc);
    kitty_session_close(kitty_osc);

    const char *invalid_osc_argv[] = {"/bin/sh", "-c",
        "printf '\\033]7;not-a-file-url\\007'; printf BADREADY; sleep 30", NULL};
    kitty_session *invalid_osc = kitty_session_create(
        e, 5, 60, invalid_osc_argv, NULL, err, sizeof err);
    assert(invalid_osc);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(invalid_osc);
        char *t = kitty_session_text(invalid_osc);
        int ready = t && strstr(t, "BADREADY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    reported_cwd = kitty_session_reported_cwd(invalid_osc);
    assert(reported_cwd && strcmp(reported_cwd, "") == 0);
    free(reported_cwd);
    kitty_session_close(invalid_osc);

    char cwd_template[] = "/private/tmp/kitmux-cwd-XXXXXX";
    char *test_cwd = mkdtemp(cwd_template);
    assert(test_cwd);
    const char *pwd_argv[] = {"/bin/sh", "-c", "pwd; sleep 30", NULL};
    kitty_session *cwd_session = kitty_session_create_with_cwd(
        e, 5, 100, pwd_argv, test_cwd, NULL, err, sizeof err);
    assert(cwd_session);
    int saw_test_cwd = 0;
    for (int i = 0; i < 400 && !saw_test_cwd; i++) {
        kitty_session_pump(cwd_session);
        char *t = kitty_session_text(cwd_session);
        saw_test_cwd = t && strstr(t, test_cwd);
        free(t);
        if (!saw_test_cwd) usleep(10000);
    }
    assert(saw_test_cwd);
    kitty_session_close(cwd_session);

    char inherited_cwd[PATH_MAX];
    assert(getcwd(inherited_cwd, sizeof inherited_cwd));
    kitty_session *fallback_session = kitty_session_create_with_cwd(
        e, 5, 100, pwd_argv, "/definitely/missing/kitmux-cwd",
        NULL, err, sizeof err);
    assert(fallback_session);
    int saw_fallback = 0;
    for (int i = 0; i < 400 && !saw_fallback; i++) {
        kitty_session_pump(fallback_session);
        char *t = kitty_session_text(fallback_session);
        saw_fallback = t && strstr(t, inherited_cwd);
        free(t);
        if (!saw_fallback) usleep(10000);
    }
    assert(saw_fallback);
    kitty_session_close(fallback_session);

    // A requested cwd that goes through symlinks (macOS /tmp ->
    // /private/tmp) must survive as the child's logical $PWD, not the
    // resolved physical path, or saved paths never round-trip on restore.
    char logical_template[] = "/tmp/kitmux-lgcl-XXXXXX";
    char *logical_cwd = mkdtemp(logical_template);
    assert(logical_cwd);
    const char *logical_argv[] = {"/bin/sh", "-c",
        "printf 'LG:%s\\n' \"$PWD\"; sleep 30", NULL};
    char expected_logical[PATH_MAX];
    snprintf(expected_logical, sizeof expected_logical, "LG:%s", logical_cwd);
    kitty_session *logical_session = kitty_session_create_with_cwd(
        e, 5, 100, logical_argv, logical_cwd, NULL, err, sizeof err);
    assert(logical_session);
    int saw_logical = 0;
    for (int i = 0; i < 400 && !saw_logical; i++) {
        kitty_session_pump(logical_session);
        char *t = kitty_session_text(logical_session);
        saw_logical = t && strstr(t, expected_logical);
        free(t);
        if (!saw_logical) usleep(10000);
    }
    assert(saw_logical);
    kitty_session_close(logical_session);
    rmdir(logical_cwd);

    char zshrc_path[PATH_MAX];
    snprintf(zshrc_path, sizeof zshrc_path, "%s/.zshrc", test_cwd);
    write_file(zshrc_path, "");
    char home_override[PATH_MAX + 6];
    snprintf(home_override, sizeof home_override, "HOME=%s", test_cwd);
    const char *zsh_env[] = {home_override, NULL};
    const char *zsh_argv[] = {"/bin/zsh", "-il", NULL};
    kitty_session *zsh_session = kitty_session_create_with_options(
        e, 5, 100, zsh_argv, test_cwd, zsh_env, NULL, err, sizeof err);
    assert(zsh_session);
    int zsh_reported = 0, zsh_prompt_defaulted = 0;
    for (int i = 0; i < 400 && (!zsh_reported || !zsh_prompt_defaulted); i++) {
        kitty_session_pump(zsh_session);
        reported_cwd = kitty_session_reported_cwd(zsh_session);
        zsh_reported = reported_cwd && reported_cwd[0] == '/';
        free(reported_cwd);
        char *t = kitty_session_text(zsh_session);
        zsh_prompt_defaulted = t && strstr(t, "~ %") && !strstr(t, "@");
        free(t);
        if (!zsh_reported || !zsh_prompt_defaulted) usleep(10000);
    }
    assert(zsh_reported);
    if (!zsh_prompt_defaulted) {
        char *t = kitty_session_text(zsh_session);
        fprintf(stderr, "default zsh prompt screen: %s\n", t ? t : "(null)");
        free(t);
    }
    assert(zsh_prompt_defaulted);
    kitty_session_close(zsh_session);

    // The product default sits underneath user configuration: any prompt set
    // by .zshrc (including prompt frameworks) must survive unchanged.
    write_file(zshrc_path, "PS1='CUSTOM %# '\n");
    kitty_session *custom_zsh_session = kitty_session_create_with_options(
        e, 5, 100, zsh_argv, test_cwd, zsh_env, NULL, err, sizeof err);
    assert(custom_zsh_session);
    int custom_prompt_preserved = 0;
    for (int i = 0; i < 400 && !custom_prompt_preserved; i++) {
        kitty_session_pump(custom_zsh_session);
        char *t = kitty_session_text(custom_zsh_session);
        custom_prompt_preserved = t && strstr(t, "CUSTOM %")
            && !strstr(t, "~ %");
        free(t);
        if (!custom_prompt_preserved) usleep(10000);
    }
    if (!custom_prompt_preserved) {
        char *t = kitty_session_text(custom_zsh_session);
        fprintf(stderr, "custom zsh prompt screen: %s\n", t ? t : "(null)");
        free(t);
    }
    assert(custom_prompt_preserved);
    kitty_session_close(custom_zsh_session);
    unlink(zshrc_path);
    rmdir(test_cwd);

    // v0.3 paste. Plain mode: \n normalizes to \r, so `read` completes.
    const char *read_argv[] = {"/bin/sh", "-c",
        "printf PROMPT; read line; printf 'GOT-%s' \"$line\"", NULL};
    kitty_session *ps = kitty_session_create(e, 10, 60, read_argv, NULL, err, sizeof err);
    assert(ps);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(ps);
        char *t = kitty_session_text(ps);
        int ready = t && strstr(t, "PROMPT");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    kitty_session_paste(ps, (const uint8_t *)"hello\n", 6);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(ps);
        char *t = kitty_session_text(ps);
        int done = t && strstr(t, "GOT-hello");
        free(t);
        if (done) break;
        usleep(10000);
    }
    char *ptext = kitty_session_text(ps);
    assert(ptext && strstr(ptext, "GOT-hello"));
    free(ptext);
    kitty_session_close(ps);

    // Bracketed mode: the child sees the paste wrapped in 200~/201~.
    const char *brk_argv[] = {"/bin/sh", "-c",
        "stty raw -echo; printf '\\033[?2004h'; printf READY; exec cat -v", NULL};
    kitty_session *bs = kitty_session_create(e, 10, 60, brk_argv, NULL, err, sizeof err);
    assert(bs);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(bs);
        char *t = kitty_session_text(bs);
        int ready = t && strstr(t, "READY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    kitty_session_paste(bs, (const uint8_t *)"hi", 2);
    int wrapped = 0;
    for (int i = 0; i < 400 && !wrapped; i++) {
        kitty_session_pump(bs);
        char *t = kitty_session_text(bs);
        wrapped = t && strstr(t, "200~hi") && strstr(t, "201~");
        free(t);
        if (!wrapped) usleep(10000);
    }
    assert(wrapped);
    kitty_session_close(bs);

    // v0.4 scrollback: viewport offset, clamping, snap-on-keypress, clear.
    const char *hist_argv[] = {"/bin/sh", "-c",
        "i=1; while [ $i -le 200 ]; do echo line-$i; i=$((i+1)); done; "
        "printf HISTREADY; read x", NULL};
    kitty_session *hs = kitty_session_create(e, 10, 60, hist_argv, NULL, err, sizeof err);
    assert(hs);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(hs);
        char *t = kitty_session_text(hs);
        int ready = t && strstr(t, "HISTREADY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    assert(kitty_session_scrolled_by(hs) == 0);
    kitty_session_scroll(hs, 5);
    assert(kitty_session_scrolled_by(hs) == 5);
    kitty_session_scroll(hs, 1000000);          // clamps to the history size
    unsigned int at_top = kitty_session_scrolled_by(hs);
    assert(at_top > 5);
    kitty_session_scroll(hs, 10);
    assert(kitty_session_scrolled_by(hs) == at_top);
    kitty_session_scroll(hs, -2);
    assert(kitty_session_scrolled_by(hs) == at_top - 2);
    // A key press while scrolled snaps to the bottom (kitty keys.c parity).
    kitty_key_event a_press = { .key = 'a', .action = 1, .text = "a" };
    n = kitty_session_encode_key(hs, &a_press, keybuf, sizeof keybuf);
    assert(n > 0);
    assert(kitty_session_scrolled_by(hs) == 0);
    // clear_scrollback drops the history: scrolling up again goes nowhere.
    kitty_session_scroll(hs, 5);
    assert(kitty_session_scrolled_by(hs) == 5);
    kitty_session_clear_scrollback(hs);
    assert(kitty_session_scrolled_by(hs) == 0);
    kitty_session_scroll(hs, 5);
    assert(kitty_session_scrolled_by(hs) == 0);
    kitty_session_close(hs);

    // v0.10 search: literal case-insensitive matching/counting, automatic
    // viewport movement, navigation, clearing, and per-session isolation.
    const char *search_argv[] = {"/bin/sh", "-c",
        "i=1; while [ $i -le 40 ]; do "
        "if [ $i -eq 5 ]; then echo 'Needle needle'; "
        "elif [ $i -eq 20 ]; then echo NEEDLE; "
        "elif [ $i -eq 35 ]; then echo needle; "
        "else echo filler-$i; fi; i=$((i+1)); done; "
        "stty -echo; printf SEARCHREADY; while read x; do echo \"$x\"; done", NULL};
    kitty_session *search = kitty_session_create(
        e, 6, 60, search_argv, NULL, err, sizeof err);
    assert(search);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(search);
        char *t = kitty_session_text(search);
        int ready = t && strstr(t, "SEARCHREADY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    assert(kitty_session_scrolled_by(search) == 0);
    size_t search_count = kitty_session_search_set(search, "nEeDlE", 6);
    assert(search_count == 4);
    unsigned int first_match_offset = kitty_session_scrolled_by(search);
    assert(first_match_offset > 0);
    assert(kitty_session_search_next(search, true));
    unsigned int older_match_offset = kitty_session_scrolled_by(search);
    assert(older_match_offset > first_match_offset);
    assert(kitty_session_search_next(search, false));
    unsigned int newer_match_offset = kitty_session_scrolled_by(search);
    assert(newer_match_offset != older_match_offset);

    size_t option_count = 0;
    assert(kitty_session_search_set_options(
        search, "Needle", 6, true, false,
        &option_count, err, sizeof err));
    assert(option_count == 1 && err[0] == 0);
    assert(kitty_session_search_set_options(
        search, "n(?:eedle)", 10, false, true,
        &option_count, err, sizeof err));
    assert(option_count == 4 && err[0] == 0);
    assert(!kitty_session_search_set_options(
        search, "(a+)+$", 6, false, true,
        &option_count, err, sizeof err));
    assert(option_count == 0 && strstr(err, "nested repetition"));
    assert(kitty_session_search_set_options(
        search, "needle", 6, false, false,
        &option_count, err, sizeof err));
    kitty_session_write(search, (const uint8_t *)"needle\n", 7);
    for (int i = 0; i < 100; i++) {
        kitty_session_pump(search);
        if (kitty_session_search_refresh(search) == 5) break;
        usleep(10000);
    }
    assert(kitty_session_search_refresh(search) == 5);

    const char *search_quiet_argv[] = {"/bin/sh", "-c",
        "printf NO_MATCH_HERE; read x", NULL};
    kitty_session *search_quiet = kitty_session_create(
        e, 6, 60, search_quiet_argv, NULL, err, sizeof err);
    assert(search_quiet);
    for (int i = 0; i < 100; i++) {
        kitty_session_pump(search_quiet);
        char *t = kitty_session_text(search_quiet);
        int ready = t && strstr(t, "NO_MATCH_HERE");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    assert(kitty_session_search_set(search_quiet, "needle", 6) == 0);
    assert(kitty_session_scrolled_by(search_quiet) == 0);
    assert(!kitty_session_search_next(search_quiet, false));

    kitty_session_search_clear(search);
    assert(!kitty_session_search_next(search, false));
    assert(kitty_session_search_set(search, "", 0) == 0);
    const char invalid_query[] = {(char)0xff};
    assert(kitty_session_search_set(search, invalid_query, sizeof invalid_query) == 0);
    kitty_session_close(search_quiet);
    kitty_session_close(search);

    // v0.5 selection: exact boundaries, clearing, history coordinates, and
    // per-session isolation all delegate to kitty's Screen.
    const char *selection_argv[] = {"/bin/sh", "-c",
        "printf 'abcdef\\r\\nsecond line\\r\\nthird line\\r\\n'; read x", NULL};
    kitty_session *ss = kitty_session_create(
        e, 5, 20, selection_argv, NULL, err, sizeof err);
    assert(ss);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(ss);
        char *t = kitty_session_text(ss);
        int ready = t && strstr(t, "third line");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    kitty_session_selection_start(ss, 0, 0, true, 0);
    kitty_session_selection_update(ss, 2, 0, false, true);
    char *selected = kitty_session_selection_text(ss);
    assert(selected && strcmp(selected, "abc") == 0);
    free(selected);

    kitty_session_selection_start(ss, 2, 0, false, 0);
    kitty_session_selection_update(ss, 0, 0, true, true);
    selected = kitty_session_selection_text(ss);
    assert(selected && strcmp(selected, "abc") == 0);
    free(selected);

    kitty_session_selection_start(ss, 0, 0, true, 0);
    kitty_session_selection_update(ss, 5, 1, false, true);
    selected = kitty_session_selection_text(ss);
    assert(selected && strcmp(selected, "abcdef\nsecond") == 0);
    free(selected);

    kitty_session_selection_clear(ss);
    selected = kitty_session_selection_text(ss);
    assert(selected && strcmp(selected, "") == 0);
    free(selected);

    kitty_session *other = kitty_session_create(
        e, 5, 20, selection_argv, NULL, err, sizeof err);
    assert(other);
    selected = kitty_session_selection_text(other);
    assert(selected && strcmp(selected, "") == 0);
    free(selected);
    kitty_session_selection_start(ss, 0, 2, true, 0);
    kitty_session_selection_update(ss, 4, 2, false, true);
    selected = kitty_session_selection_text(ss);
    assert(selected && strcmp(selected, "third") == 0);
    free(selected);
    selected = kitty_session_selection_text(other);
    assert(selected && strcmp(selected, "") == 0);
    free(selected);
    kitty_session_close(other);

    // v0.12 word/line selection: row 1 is "second line". A word-mode start
    // (double-click) inside "second" selects the whole word; a line-mode
    // start (triple-click) selects the whole line. kitty owns the boundaries.
    kitty_session_selection_start(ss, 2, 1, true, 1);  // word
    selected = kitty_session_selection_text(ss);
    assert(selected && strcmp(selected, "second") == 0);
    free(selected);
    kitty_session_selection_start(ss, 2, 1, true, 2);  // line
    selected = kitty_session_selection_text(ss);
    assert(selected && strcmp(selected, "second line") == 0);
    free(selected);
    kitty_session_close(ss);

    const char *wrapped_argv[] = {"/bin/sh", "-c",
        "printf 'abcdefghijklmnop'; read x", NULL};
    kitty_session *wrapped_session = kitty_session_create(
        e, 5, 10, wrapped_argv, NULL, err, sizeof err);
    assert(wrapped_session);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(wrapped_session);
        char *t = kitty_session_text(wrapped_session);
        int ready = t && strstr(t, "klmnop");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    kitty_session_selection_start(wrapped_session, 0, 0, true, 0);
    kitty_session_selection_update(wrapped_session, 5, 1, false, true);
    selected = kitty_session_selection_text(wrapped_session);
    assert(selected && strcmp(selected, "abcdefghijklmnop") == 0);
    free(selected);
    kitty_session_close(wrapped_session);

    const char *unicode_argv[] = {"/bin/sh", "-c",
        "printf 'A界éZ'; read x", NULL};
    kitty_session *unicode_session = kitty_session_create(
        e, 3, 20, unicode_argv, NULL, err, sizeof err);
    assert(unicode_session);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(unicode_session);
        char *t = kitty_session_text(unicode_session);
        int ready = t && strstr(t, "Z");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    kitty_session_selection_start(unicode_session, 1, 0, true, 0);
    kitty_session_selection_update(unicode_session, 3, 0, false, true);
    selected = kitty_session_selection_text(unicode_session);
    assert(selected && strcmp(selected, "界é") == 0);
    free(selected);
    kitty_session_close(unicode_session);

    const char *selection_history_argv[] = {"/bin/sh", "-c",
        "i=1; while [ $i -le 30 ]; do printf 'history-%02d\\r\\n' $i; "
        "i=$((i+1)); done; read x", NULL};
    kitty_session *shs = kitty_session_create(
        e, 5, 20, selection_history_argv, NULL, err, sizeof err);
    assert(shs);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(shs);
        char *t = kitty_session_text(shs);
        int ready = t && strstr(t, "history-30");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    kitty_session_scroll(shs, 5);
    assert(kitty_session_scrolled_by(shs) == 5);
    kitty_session_selection_start(shs, 0, 0, true, 0);
    kitty_session_selection_update(shs, 9, 0, false, true);
    selected = kitty_session_selection_text(shs);
    assert(selected && strncmp(selected, "history-", 8) == 0);
    char history_selection[32];
    snprintf(history_selection, sizeof history_selection, "%s", selected);
    free(selected);
    kitty_session_scroll(shs, -3);
    selected = kitty_session_selection_text(shs);
    assert(selected && strcmp(selected, history_selection) == 0);
    free(selected);
    kitty_session_close(shs);

    // Alt screen: scroll becomes arrow keys to the child (kitty fake_scroll).
    const char *alt_argv[] = {"/bin/sh", "-c",
        "stty raw -echo; printf '\\033[?1049h'; printf ALTREADY; exec cat -v", NULL};
    kitty_session *as = kitty_session_create(e, 10, 60, alt_argv, NULL, err, sizeof err);
    assert(as);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(as);
        char *t = kitty_session_text(as);
        int ready = t && strstr(t, "ALTREADY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    kitty_session_scroll(as, 1);
    int saw_up = 0;
    for (int i = 0; i < 400 && !saw_up; i++) {
        kitty_session_pump(as);
        char *t = kitty_session_text(as);
        saw_up = t && strstr(t, "^[[A");
        free(t);
        if (!saw_up) usleep(10000);
    }
    assert(saw_up);
    assert(kitty_session_scrolled_by(as) == 0);  // no viewport scroll in alt screen
    kitty_session_scroll(as, -1);
    int saw_down = 0;
    for (int i = 0; i < 400 && !saw_down; i++) {
        kitty_session_pump(as);
        char *t = kitty_session_text(as);
        saw_down = t && strstr(t, "^[[B");
        free(t);
        if (!saw_down) usleep(10000);
    }
    assert(saw_down);
    kitty_session_close(as);

    // v0.7 mouse reporting: kitty's send_mouse_event gates on the tracking
    // DECSETs and encodes in the application's protocol. A cat -v child
    // echoes forwarded sequences back as visible screen text.
    const char *sgr_argv[] = {"/bin/sh", "-c",
        "stty raw -echo; printf '\\033[?1002h\\033[?1006h'; "
        "printf MOUSEREADY; exec cat -v", NULL};
    // 120 columns: five echoed SGR sequences must not hit the wrap margin,
    // or strstr sees them split across screen lines.
    kitty_session *ms = kitty_session_create(e, 10, 120, sgr_argv, NULL, err, sizeof err);
    assert(ms);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(ms);
        char *t = kitty_session_text(ms);
        int ready = t && strstr(t, "MOUSEREADY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    // Press, drag, release, wheel-up, and shift+right press are all consumed
    // under 1002 (motion tracking) and SGR-encoded with 1-based coordinates.
    assert(kitty_session_mouse_event(ms, 10, 5, 1, KITTY_MOUSE_PRESS, 0, 0, 0) == 1);
    assert(kitty_session_mouse_event(ms, 12, 5, 1, KITTY_MOUSE_DRAG, 0, 0, 0) == 1);
    assert(kitty_session_mouse_event(ms, 12, 5, 1, KITTY_MOUSE_RELEASE, 0, 0, 0) == 1);
    assert(kitty_session_mouse_event(ms, 10, 5, 4, KITTY_MOUSE_PRESS, 0, 0, 0) == 1);
    assert(kitty_session_mouse_event(ms, 10, 5, 3, KITTY_MOUSE_PRESS, 0x1, 0, 0) == 1);
    int saw_mouse = 0;
    for (int i = 0; i < 400 && !saw_mouse; i++) {
        kitty_session_pump(ms);
        char *t = kitty_session_text(ms);
        saw_mouse = t && strstr(t, "^[[<0;11;6M") && strstr(t, "^[[<32;13;6M")
            && strstr(t, "^[[<0;13;6m") && strstr(t, "^[[<64;11;6M")
            && strstr(t, "^[[<6;11;6M");
        free(t);
        if (!saw_mouse) usleep(10000);
    }
    assert(saw_mouse);
    kitty_session_close(ms);

    // No tracking mode enabled: the offer is declined with no side effects.
    const char *plain_argv[] = {"/bin/sh", "-c", "printf PLAINREADY; sleep 30", NULL};
    kitty_session *pm = kitty_session_create(e, 10, 60, plain_argv, NULL, err, sizeof err);
    assert(pm);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(pm);
        char *t = kitty_session_text(pm);
        int ready = t && strstr(t, "PLAINREADY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    assert(kitty_session_mouse_event(pm, 0, 0, 1, KITTY_MOUSE_PRESS, 0, 0, 0) == 0);
    assert(kitty_session_mouse_event(pm, 0, 0, 1, KITTY_MOUSE_DRAG, 0, 0, 0) == 0);
    kitty_session_close(pm);

    // BUTTON_MODE (1000): press/release forwarded, drags dropped.
    const char *btn_argv[] = {"/bin/sh", "-c",
        "stty raw -echo; printf '\\033[?1000h\\033[?1006h'; "
        "printf BTNREADY; exec cat -v", NULL};
    kitty_session *bm = kitty_session_create(e, 10, 60, btn_argv, NULL, err, sizeof err);
    assert(bm);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(bm);
        char *t = kitty_session_text(bm);
        int ready = t && strstr(t, "BTNREADY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    assert(kitty_session_mouse_event(bm, 2, 2, 1, KITTY_MOUSE_PRESS, 0, 0, 0) == 1);
    assert(kitty_session_mouse_event(bm, 4, 2, 1, KITTY_MOUSE_DRAG, 0, 0, 0) == 0);
    assert(kitty_session_mouse_event(bm, 4, 2, 1, KITTY_MOUSE_RELEASE, 0, 0, 0) == 1);
    int saw_btn = 0;
    for (int i = 0; i < 400 && !saw_btn; i++) {
        kitty_session_pump(bm);
        char *t = kitty_session_text(bm);
        saw_btn = t && strstr(t, "^[[<0;3;3M") && strstr(t, "^[[<0;5;3m")
            && !strstr(t, "^[[<32;");
        free(t);
        if (!saw_btn) usleep(10000);
    }
    assert(saw_btn);
    kitty_session_close(bm);

    // Legacy protocol (1000 without 1006): byte-oriented 32-offset encoding.
    // Press at cell (0,0) reaches the child as "M !!" (M, 0+32, 1+32, 1+32).
    const char *legacy_argv[] = {"/bin/sh", "-c",
        "stty raw -echo; printf '\\033[?1000h'; printf LEGACYREADY; exec cat -v", NULL};
    kitty_session *lm = kitty_session_create(e, 10, 60, legacy_argv, NULL, err, sizeof err);
    assert(lm);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(lm);
        char *t = kitty_session_text(lm);
        int ready = t && strstr(t, "LEGACYREADY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    assert(kitty_session_mouse_event(lm, 0, 0, 1, KITTY_MOUSE_PRESS, 0, 0, 0) == 1);
    int saw_legacy = 0;
    for (int i = 0; i < 400 && !saw_legacy; i++) {
        kitty_session_pump(lm);
        char *t = kitty_session_text(lm);
        saw_legacy = t && strstr(t, "M !!");
        free(t);
        if (!saw_legacy) usleep(10000);
    }
    assert(saw_legacy);
    kitty_session_close(lm);

    // Real application: less --mouse enables 1000+1006 and scrolls the view
    // on forwarded wheel-down events.
    char less_file[] = "/private/tmp/kitmux-less-XXXXXX";
    int less_fd = mkstemp(less_file);
    assert(less_fd >= 0);
    FILE *less_f = fdopen(less_fd, "w");
    assert(less_f);
    for (int i = 1; i <= 100; i++) fprintf(less_f, "L-%03d\n", i);
    fclose(less_f);
    char less_cmd[PATH_MAX + 32];
    snprintf(less_cmd, sizeof less_cmd, "exec less --mouse %s", less_file);
    const char *less_argv[] = {"/bin/sh", "-c", less_cmd, NULL};
    kitty_session *ls = kitty_session_create(e, 5, 40, less_argv, NULL, err, sizeof err);
    assert(ls);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(ls);
        char *t = kitty_session_text(ls);
        int ready = t && strstr(t, "L-001");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    // less may enable tracking a beat after the first paint; retry the offer.
    int less_wheel = 0;
    for (int i = 0; i < 400 && !less_wheel; i++) {
        less_wheel = kitty_session_mouse_event(ls, 2, 2, 5, KITTY_MOUSE_PRESS, 0, 0, 0);
        if (!less_wheel) { kitty_session_pump(ls); usleep(10000); }
    }
    assert(less_wheel);
    assert(kitty_session_mouse_event(ls, 2, 2, 5, KITTY_MOUSE_PRESS, 0, 0, 0) == 1);
    assert(kitty_session_mouse_event(ls, 2, 2, 5, KITTY_MOUSE_PRESS, 0, 0, 0) == 1);
    int less_scrolled = 0;
    for (int i = 0; i < 400 && !less_scrolled; i++) {
        kitty_session_pump(ls);
        char *t = kitty_session_text(ls);
        less_scrolled = t && !strstr(t, "L-001") && strstr(t, "L-0");
        free(t);
        if (!less_scrolled) usleep(10000);
    }
    assert(less_scrolled);
    kitty_session_close(ls);
    unlink(less_file);

    // v0.9 notifications: kitty's parser dispatches OSC 9/99/777 to
    // desktop_notify; libkitty surfaces the parsed (title, body) through
    // on_notification, in arrival order, from pump.
    kitty_session_callbacks note_cbs = { .on_notification = on_note };
    const char *notify_argv[] = {"/bin/sh", "-c",
        "printf '\\033]777;notify;Build done;line1;line2\\007'; "
        "printf '\\033]9;simple message\\007'; "
        "printf '\\033]99;bare message\\007'; "
        "printf '\\033]99;p=body;just body\\007'; "
        "printf '\\033]99;p=title:e=1;c2VjcmV0\\007'; "
        "printf NOTIFYREADY; sleep 30", NULL};
    kitty_session *ns = kitty_session_create(
        e, 5, 60, notify_argv, &note_cbs, err, sizeof err);
    assert(ns);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(ns);
        char *t = kitty_session_text(ns);
        int ready = t && strstr(t, "NOTIFYREADY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    assert(note_count == 5);
    assert(strcmp(note_log[0], "Build done|line1;line2") == 0);  // 777: body keeps ';'
    assert(strcmp(note_log[1], "simple message|") == 0);         // 9: title only
    assert(strcmp(note_log[2], "bare message|") == 0);           // 99 bare shape
    assert(strcmp(note_log[3], "|just body") == 0);              // 99 p=body
    assert(strcmp(note_log[4], "secret|") == 0);                 // 99 e=1 base64
    kitty_session_close(ns);

    // Unsupported or malformed sequences are dropped quietly and never
    // corrupt later notifications: unknown 777 subcommand, chunked 99
    // (d=0), malformed metadata, unsupported payload type, invalid base64,
    // and OSC 1337 all fire nothing; the trailing OSC 9 still arrives.
    note_count = 0;
    const char *drop_argv[] = {"/bin/sh", "-c",
        "printf '\\033]777;something-else;x;y\\007'; "
        "printf '\\033]99;i=1:d=0;chunk\\007'; "
        "printf '\\033]99;foo:bar;x\\007'; "
        "printf '\\033]99;p=icon;x\\007'; "
        "printf '\\033]99;p=title:e=1;.!.\\007'; "
        "printf '\\033]1337;File=x\\007'; "
        "printf '\\033]9;sentinel\\007'; "
        "printf DROPREADY; sleep 30", NULL};
    kitty_session *ds = kitty_session_create(
        e, 5, 60, drop_argv, &note_cbs, err, sizeof err);
    assert(ds);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(ds);
        char *t = kitty_session_text(ds);
        int ready = t && strstr(t, "DROPREADY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    assert(note_count == 1);
    assert(strcmp(note_log[0], "sentinel|") == 0);
    kitty_session_close(ds);

    // Oversized payloads are truncated to the 2048-char field cap, and a
    // session with no on_notification callback survives notifications.
    note_count = 0;
    const char *big_argv[] = {"/bin/sh", "-c",
        "printf '\\033]9;'; i=0; while [ $i -lt 300 ]; do "
        "printf 'ABCDEFGHIJ'; i=$((i+1)); done; printf '\\007'; "
        "printf BIGREADY; sleep 30", NULL};
    kitty_session *bigs = kitty_session_create(
        e, 5, 60, big_argv, &note_cbs, err, sizeof err);
    assert(bigs);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(bigs);
        char *t = kitty_session_text(bigs);
        int ready = t && strstr(t, "BIGREADY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    assert(note_count == 1);
    assert(strlen(note_log[0]) == 2048 + 1);  // 2048 title chars + '|'
    assert(strncmp(note_log[0], "ABCDEFGHIJ", 10) == 0);
    kitty_session_close(bigs);

    const char *nocb_argv[] = {"/bin/sh", "-c",
        "printf '\\033]9;ignored\\007'; printf NOCBREADY; sleep 30", NULL};
    kitty_session *nocb = kitty_session_create(
        e, 5, 60, nocb_argv, NULL, err, sizeof err);
    assert(nocb);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(nocb);
        char *t = kitty_session_text(nocb);
        int ready = t && strstr(t, "NOCBREADY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    kitty_session_close(nocb);

    // v0.11 user vars: OSC 1337 SetUserVar is captured and base64-decoded;
    // a bare name clears it (empty value); a non-SetUserVar OSC 1337 and a
    // malformed-base64 payload fire nothing; libkitty forwards every var key
    // generically (the host decides which it cares about).
    kitty_session_callbacks uvar_cbs = { .on_user_var = on_uvar };
    const char *uvar_argv[] = {"/bin/sh", "-c",
        "V=$(printf 'claude --resume X' | base64); "
        "printf '\\033]1337;SetUserVar=kitmux_resume=%s\\007' \"$V\"; "
        "printf '\\033]1337;SetUserVar=kitmux_resume\\007'; "     // clear
        "printf '\\033]1337;File=x\\007'; "                       // non-SetUserVar
        "printf '\\033]1337;SetUserVar=kitmux_resume=A\\007'; "   // malformed b64
        "W=$(printf 'hello' | base64); "
        "printf '\\033]1337;SetUserVar=OTHER=%s\\007' \"$W\"; "   // generic key
        "printf UVREADY; sleep 30", NULL};
    kitty_session *uvs = kitty_session_create(
        e, 5, 60, uvar_argv, &uvar_cbs, err, sizeof err);
    assert(uvs);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(uvs);
        char *t = kitty_session_text(uvs);
        int ready = t && strstr(t, "UVREADY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    assert(uvar_count == 3);
    assert(strcmp(uvar_log[0], "kitmux_resume|claude --resume X") == 0);
    assert(strcmp(uvar_log[1], "kitmux_resume|") == 0);   // bare name clears
    assert(strcmp(uvar_log[2], "OTHER|hello") == 0);      // forwarded generically
    kitty_session_close(uvs);

    // v0.14 / 03B spike: bounded plain-text export includes main-screen
    // history but leaves viewport, selection, and search state untouched.
    const char *snapshot_argv[] = {"/bin/sh", "-c",
        "i=1; while [ $i -le 80 ]; do printf 'export-%02d\\r\\n' $i; "
        "i=$((i+1)); done; printf SNAPREADY; sleep 30", NULL};
    kitty_session *snapshot = kitty_session_create(
        e, 6, 40, snapshot_argv, NULL, err, sizeof err);
    assert(snapshot);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(snapshot);
        char *t = kitty_session_text(snapshot);
        int ready = t && strstr(t, "SNAPREADY");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    kitty_session_scroll(snapshot, 4);
    kitty_session_selection_start(snapshot, 0, 0, true, 0);
    kitty_session_selection_update(snapshot, 8, 0, false, true);
    char *selection_before_export = kitty_session_selection_text(snapshot);
    assert(selection_before_export && selection_before_export[0]);
    assert(kitty_session_search_set(snapshot, "export-20", 9) == 1);
    unsigned int viewport_before_export = kitty_session_scrolled_by(snapshot);

    char *exported = kitty_session_scrollback_text(snapshot, 200, 16384);
    assert(exported);
    assert(strstr(exported, "export-01"));
    assert(strstr(exported, "export-80"));
    assert(strstr(exported, "SNAPREADY"));
    assert(!strchr(exported, '\033'));                 // plain text, never VT
    assert(kitty_session_scrolled_by(snapshot) == viewport_before_export);
    char *selection_after_export = kitty_session_selection_text(snapshot);
    assert(selection_after_export);
    assert(strcmp(selection_before_export, selection_after_export) == 0);
    free(selection_before_export); free(selection_after_export); free(exported);
    assert(kitty_session_search_next(snapshot, false)); // marker survived export
    kitty_session_search_clear(snapshot);

    exported = kitty_session_scrollback_text(snapshot, 3, 16384);
    assert(exported);
    int newline_count = 0;
    for (const char *p = exported; *p; p++) newline_count += *p == '\n';
    assert(newline_count <= 2);                         // at most three lines
    free(exported);
    exported = kitty_session_scrollback_text(snapshot, 200, 17);
    assert(exported && strlen(exported) <= 17);         // hard UTF-8 byte cap
    free(exported);
    exported = kitty_session_scrollback_text(snapshot, 0, 100);
    assert(exported && !*exported);                     // clean zero-cap fallback
    free(exported);
    kitty_session_close(snapshot);

    // Alternate-screen content is intentionally not mislabeled as restorable
    // shell history; the spike returns a clean empty snapshot without moving it.
    const char *alt_export_argv[] = {"/bin/sh", "-c",
        "printf '\\033[?1049hALTEXPORT'; sleep 30", NULL};
    kitty_session *alt_export = kitty_session_create(
        e, 6, 40, alt_export_argv, NULL, err, sizeof err);
    assert(alt_export);
    for (int i = 0; i < 400; i++) {
        kitty_session_pump(alt_export);
        char *t = kitty_session_text(alt_export);
        int ready = t && strstr(t, "ALTEXPORT");
        free(t);
        if (ready) break;
        usleep(10000);
    }
    unsigned int alt_viewport = kitty_session_scrolled_by(alt_export);
    exported = kitty_session_scrollback_text(alt_export, 100, 4096);
    assert(exported && !*exported);
    assert(kitty_session_scrolled_by(alt_export) == alt_viewport);
    free(exported);
    kitty_session_close(alt_export);

    // Inert replay proof in clean zsh and bash (startup integration disabled):
    // a command-looking snapshot is rendered through Screen output, never sent
    // to either shell. Both shells remain interactive afterward.
    const char *shell_names[] = {"zsh", "bash"};
    const char *replay_zsh_argv[] = {"/bin/zsh", "-f", NULL};
    const char *replay_bash_argv[] = {
        "/bin/bash", "--noprofile", "--norc", NULL};
    const char *const *shell_argvs[] = {replay_zsh_argv, replay_bash_argv};
    kitty_session_callbacks replay_cbs = { .on_title = on_title };
    for (size_t shell_index = 0; shell_index < 2; shell_index++) {
        char sentinel[128], replay[512];
        snprintf(sentinel, sizeof sentinel, "/tmp/kitmux-03b-%d-%s",
                 getpid(), shell_names[shell_index]);
        unlink(sentinel);
        snprintf(replay, sizeof replay,
                 "--- restored history ---\n"
                 "printf PWNED > %s\n"
                 "\033]2;forbidden-title\007restored-safe\n",
                 sentinel);
        last_title[0] = 0;
        kitty_session *replayed = kitty_session_create(
            e, 12, 80, shell_argvs[shell_index], &replay_cbs, err, sizeof err);
        assert(replayed);
        assert(kitty_session_replay_plain_text(
            replayed, (const uint8_t *)replay, strlen(replay)));
        char *t = kitty_session_text(replayed);
        assert(t && strstr(t, "restored-safe"));
        assert(!strchr(t, '\033'));                    // controls were inert/stripped
        free(t);
        assert(access(sentinel, F_OK) != 0);            // displayed, never executed

        const char *probe = "printf 'SHELL-' ; printf '%s' $((20+22))\r";
        kitty_session_write(replayed, (const uint8_t *)probe, strlen(probe));
        int shell_ready = 0;
        for (int i = 0; i < 400 && !shell_ready; i++) {
            kitty_session_pump(replayed);
            t = kitty_session_text(replayed);
            shell_ready = t && strstr(t, "SHELL-42");
            free(t);
            if (!shell_ready) usleep(10000);
        }
        assert(shell_ready);
        assert(kitty_session_child_alive(replayed));
        assert(access(sentinel, F_OK) != 0);
        assert(strcmp(last_title, "forbidden-title") != 0);
        kitty_session_close(replayed);
    }

    // Closing a live shell that ignores polite signals must remain bounded.
    const char *stubborn_argv[] = {"/bin/sh", "-c",
        "trap '' HUP TERM; while :; do sleep 10; done", NULL};
    kitty_session *stubborn = kitty_session_create(
        e, 10, 60, stubborn_argv, NULL, err, sizeof err);
    assert(stubborn);
    usleep(50000);
    struct timespec before, after;
    clock_gettime(CLOCK_MONOTONIC, &before);
    kitty_session_close(stubborn);
    clock_gettime(CLOCK_MONOTONIC, &after);
    double close_seconds = (after.tv_sec - before.tv_sec)
        + (after.tv_nsec - before.tv_nsec) / 1e9;
    assert(close_seconds < 2.0);

    // A permanently noisy producer must yield from each pump call so another
    // session and the host event loop continue to make progress.
    const char *noisy_argv[] = {"/bin/sh", "-c", "while :; do printf x; done", NULL};
    kitty_session *noisy = kitty_session_create(
        e, 10, 60, noisy_argv, NULL, err, sizeof err);
    assert(noisy);
    usleep(50000);
    clock_gettime(CLOCK_MONOTONIC, &before);
    assert(kitty_session_pump(noisy));
    clock_gettime(CLOCK_MONOTONIC, &after);
    double pump_seconds = (after.tv_sec - before.tv_sec)
        + (after.tv_nsec - before.tv_nsec) / 1e9;
    assert(pump_seconds < 0.5);
    size_t noisy_bytes = kitty_session_last_pump_bytes(noisy);
    assert(noisy_bytes > 0 && noisy_bytes <= 65536);

    // Fairness means a quiet peer progresses while the noisy producer stays
    // continuously readable, not merely that one noisy pump eventually ends.
    const char *fair_peer_argv[] = {"/bin/sh", "-c", "printf QUIET-PEER; sleep 1", NULL};
    kitty_session *fair_peer = kitty_session_create(
        e, 10, 60, fair_peer_argv, NULL, err, sizeof err);
    assert(fair_peer);
    int quiet_progress = 0;
    for (int i = 0; i < 100 && !quiet_progress; i++) {
        assert(kitty_session_pump(noisy));
        kitty_session_pump(fair_peer);
        char *quiet_text = kitty_session_text(fair_peer);
        quiet_progress = quiet_text && strstr(quiet_text, "QUIET-PEER");
        free(quiet_text);
        if (!quiet_progress) usleep(5000);
    }
    assert(quiet_progress);
    kitty_session_close(fair_peer);
    kitty_session_close(noisy);

    test_visible_line_wrap_metadata(e, err, sizeof err);

    kitty_engine_shutdown(e);
    printf("session API OK\n");
    return 0;
}
