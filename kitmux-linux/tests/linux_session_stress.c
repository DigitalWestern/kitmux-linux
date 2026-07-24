#define _GNU_SOURCE

#include "libkitty.h"

#include <assert.h>
#include <dirent.h>
#include <errno.h>
#include <signal.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

enum {
  FLOOD_SESSION_COUNT = 16,
  CLOSE_CYCLE_COUNT = 24,
};

typedef struct session_state {
  int exited;
  int exit_status;
} session_state;

static void on_child_exit(void *userdata, int exit_status) {
  session_state *state = userdata;
  state->exited++;
  state->exit_status = exit_status;
}

static size_t open_fd_count(void) {
  DIR *directory = opendir("/proc/self/fd");
  assert(directory);

  size_t count = 0;
  while (readdir(directory)) {
    count++;
  }
  assert(closedir(directory) == 0);
  assert(count >= 2);
  return count - 2;
}

static void assert_child_reaped(pid_t pid) {
  int status = 0;
  for (int attempt = 0; attempt < 200; attempt++) {
    errno = 0;
    pid_t waited = waitpid(pid, &status, WNOHANG);
    if (waited == -1 && errno == ECHILD) {
      errno = 0;
      assert(kill(pid, 0) == -1);
      assert(errno == ESRCH);
      return;
    }
    assert(waited == 0 || (waited == -1 && errno == ECHILD));
    usleep(10000);
  }
  fprintf(stderr, "child %ld was not reaped within two seconds\n", (long)pid);
  abort();
}

static void wait_for_text(kitty_session *session, const char *needle) {
  for (int attempt = 0; attempt < 500; attempt++) {
    kitty_session_pump(session);
    char *text = kitty_session_text(session);
    bool found = text && strstr(text, needle);
    free(text);
    if (found) {
      return;
    }
    usleep(10000);
  }
  fprintf(stderr, "session did not produce marker: %s\n", needle);
  abort();
}

static void warm_up(kitty_engine *engine, char *error, size_t error_size) {
  const char *argv[] = {"/bin/sh", "-c", "printf warm-up", NULL};
  kitty_session *session =
      kitty_session_create(engine, 4, 40, argv, NULL, error, error_size);
  assert(session);
  wait_for_text(session, "warm-up");
  pid_t pid = kitty_session_child_pid(session);
  assert(pid > 0);
  kitty_session_close(session);
  assert_child_reaped(pid);
}

static void test_many_session_flood(kitty_engine *engine, char *error,
                                    size_t error_size) {
  kitty_session *sessions[FLOOD_SESSION_COUNT] = {0};
  session_state states[FLOOD_SESSION_COUNT] = {0};
  pid_t pids[FLOOD_SESSION_COUNT] = {0};
  char commands[FLOOD_SESSION_COUNT][320];
  char markers[FLOOD_SESSION_COUNT][64];
  size_t total_bytes = 0;

  for (int index = 0; index < FLOOD_SESSION_COUNT; index++) {
    snprintf(markers[index], sizeof markers[index], "KITMUX-FLOOD-%02d", index);
    snprintf(commands[index], sizeof commands[index],
             "i=0; while [ \"$i\" -lt 1000 ]; do "
             "printf 'pane-%02d-0123456789abcdefghijklmnopqrstuvwxyz\\n'; "
             "i=$((i+1)); done; printf '%s\\n'; exit %d",
             index, markers[index], index % 7);
    const char *argv[] = {"/bin/sh", "-c", commands[index], NULL};
    kitty_session_callbacks callbacks = {
        .userdata = &states[index],
        .on_child_exit = on_child_exit,
    };
    sessions[index] = kitty_session_create(engine, 8, 80, argv, &callbacks,
                                           error, error_size);
    if (!sessions[index]) {
      fprintf(stderr, "session %d creation failed: %s\n", index, error);
      abort();
    }
    pids[index] = kitty_session_child_pid(sessions[index]);
    assert(pids[index] > 0);
  }

  for (int turn = 0; turn < 4000; turn++) {
    int exited = 0;
    for (int index = 0; index < FLOOD_SESSION_COUNT; index++) {
      kitty_session_pump(sessions[index]);
      total_bytes += kitty_session_last_pump_bytes(sessions[index]);
      exited += states[index].exited == 1;
    }
    if (exited == FLOOD_SESSION_COUNT) {
      break;
    }
    usleep(1000);
  }

  assert(total_bytes >= 500000);
  for (int index = 0; index < FLOOD_SESSION_COUNT; index++) {
    assert(states[index].exited == 1);
    assert(states[index].exit_status == index % 7);
    assert(!kitty_session_child_alive(sessions[index]));
    char *text = kitty_session_text(sessions[index]);
    assert(text && strstr(text, markers[index]));
    free(text);
    kitty_session_close(sessions[index]);
    assert_child_reaped(pids[index]);
  }

  printf("%d-session flood OK (%zu bytes pumped)\n", FLOOD_SESSION_COUNT,
         total_bytes);
}

static void test_repeated_forced_close(kitty_engine *engine, char *error,
                                       size_t error_size) {
  const char *argv[] = {
      "/bin/sh", "-c",
      "trap '' HUP TERM; printf CLOSE-READY; while :; do sleep 30; done", NULL};

  for (int cycle = 0; cycle < CLOSE_CYCLE_COUNT; cycle++) {
    kitty_session *session =
        kitty_session_create(engine, 4, 40, argv, NULL, error, error_size);
    if (!session) {
      fprintf(stderr, "close cycle %d creation failed: %s\n", cycle, error);
      abort();
    }
    pid_t pid = kitty_session_child_pid(session);
    assert(pid > 0);
    wait_for_text(session, "CLOSE-READY");
    kitty_session_close(session);
    assert_child_reaped(pid);
  }

  printf("%d forced-close cycles OK\n", CLOSE_CYCLE_COUNT);
}

int main(void) {
  char error[512] = "";
  kitty_engine_config config = {
      .kitty_src_path = getenv("KITTY_SRC"),
      .libkitty_py_path = getenv("LIBKITTY_PY"),
      .python_home = getenv("PYTHONHOME"),
      .config_path = getenv("LIBKITTY_TEST_CONFIG"),
  };
  kitty_engine *engine = kitty_engine_init(&config, error, sizeof error);
  if (!engine) {
    fprintf(stderr, "engine initialization failed: %s\n", error);
    return 1;
  }

  warm_up(engine, error, sizeof error);
  size_t baseline_fds = open_fd_count();

  test_many_session_flood(engine, error, sizeof error);
  test_repeated_forced_close(engine, error, sizeof error);

  size_t final_fds = open_fd_count();
  assert(final_fds == baseline_fds);
  kitty_engine_shutdown(engine);

  printf("FD baseline restored (%zu open)\n", baseline_fds);
  return 0;
}
