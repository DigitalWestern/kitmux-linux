/*
 * Deterministic child fixture for the Slice 2.2A keyboard harness.
 *
 * Runs on the session's PTY slave in place of a login shell. It puts the
 * line discipline into raw mode so the bytes a real terminal application
 * would see are the bytes it records, appends every read as hex to
 * KITMUX_RECORDER_LOG, and can emit a fixed escape sequence at startup so a
 * run exercises kitty's live terminal state (DECCKM, the keyboard-protocol
 * flag stack) instead of defaults only.
 *
 * Environment:
 *   KITMUX_RECORDER_LOG   required; append-only hex/marker log path
 *   KITMUX_RECORDER_INIT  optional; bytes written to the screen at startup,
 *                         "\e" for ESC (e.g. "\e[>15u", "\e[?1h")
 *   KITMUX_RECORDER_QUIT  optional; hex byte sequence that ends the fixture
 *                         after it has been recorded
 */
#define _GNU_SOURCE
#include <errno.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <termios.h>
#include <unistd.h>

#define RECORDER_TAIL_CAPACITY 64

static void write_all(int fd, const char *data, size_t length) {
  size_t written = 0;
  while (written < length) {
    ssize_t chunk = write(fd, data + written, length - written);
    if (chunk <= 0) {
      if (chunk < 0 && errno == EINTR) continue;
      return;
    }
    written += (size_t)chunk;
  }
}

// "\e" -> ESC, "\\" -> backslash; every other byte is literal.
static size_t unescape(const char *source, char *out, size_t capacity) {
  size_t length = 0;
  for (const char *p = source; *p && length + 1 < capacity; ++p) {
    if (*p != '\\' || p[1] == '\0') {
      out[length++] = *p;
      continue;
    }
    ++p;
    switch (*p) {
      case 'e': out[length++] = 0x1B; break;
      case 'n': out[length++] = '\n'; break;
      case 'r': out[length++] = '\r'; break;
      case '\\': out[length++] = '\\'; break;
      default: out[length++] = *p; break;
    }
  }
  out[length] = '\0';
  return length;
}

static size_t parse_hex(const char *source, unsigned char *out,
                        size_t capacity) {
  size_t length = 0;
  for (const char *p = source; p[0] && p[1] && length < capacity; p += 2) {
    unsigned value = 0;
    if (sscanf(p, "%2x", &value) != 1) break;
    out[length++] = (unsigned char)value;
  }
  return length;
}

int main(void) {
  const char *log_path = getenv("KITMUX_RECORDER_LOG");
  if (!log_path || !*log_path) {
    fprintf(stderr, "pty_input_recorder: KITMUX_RECORDER_LOG is required\n");
    return 2;
  }
  FILE *log = fopen(log_path, "a");
  if (!log) {
    fprintf(stderr, "pty_input_recorder: cannot open %s\n", log_path);
    return 2;
  }
  setvbuf(log, NULL, _IONBF, 0);

  // Raw mode: no echo, no canonical buffering, no CR/LF or signal rewriting,
  // so recorded bytes are exactly the encoder's bytes.
  struct termios raw;
  if (tcgetattr(STDIN_FILENO, &raw) == 0) {
    cfmakeraw(&raw);
    raw.c_cc[VMIN] = 1;
    raw.c_cc[VTIME] = 0;
    tcsetattr(STDIN_FILENO, TCSANOW, &raw);
  }

  const char *init = getenv("KITMUX_RECORDER_INIT");
  if (init && *init) {
    char bytes[128];
    size_t length = unescape(init, bytes, sizeof(bytes));
    write_all(STDOUT_FILENO, bytes, length);
  }
  // Always emit something: the host's first pump report is the harness's
  // proof that any startup mode change reached kitty's Screen.
  write_all(STDOUT_FILENO, "recorder-ready\r\n", 16);

  unsigned char quit[RECORDER_TAIL_CAPACITY];
  size_t quit_length = 0;
  const char *quit_hex = getenv("KITMUX_RECORDER_QUIT");
  if (quit_hex && *quit_hex) {
    quit_length = parse_hex(quit_hex, quit, sizeof(quit));
  }

  fprintf(log, "ready\n");
  unsigned char tail[RECORDER_TAIL_CAPACITY] = {0};
  size_t tail_length = 0;
  unsigned char buffer[4096];
  unsigned long long total = 0;
  for (;;) {
    ssize_t count = read(STDIN_FILENO, buffer, sizeof(buffer));
    if (count < 0) {
      if (errno == EINTR) continue;
      break;
    }
    if (count == 0) break;
    fprintf(log, "bytes ");
    for (ssize_t i = 0; i < count; ++i) fprintf(log, "%02x", buffer[i]);
    fprintf(log, "\n");
    total += (unsigned long long)count;

    // Echo the same bytes back as hex so the rendered terminal shows exactly
    // what the child received. Hex digits and CRLF cannot disturb the screen
    // the way echoing raw escape sequences would.
    char echo[3 * 4096 + 2];
    size_t used = 0;
    for (ssize_t i = 0; i < count && used + 3 < sizeof(echo); ++i) {
      used += (size_t)snprintf(echo + used, sizeof(echo) - used, "%02x",
                               buffer[i]);
    }
    if (used + 2 < sizeof(echo)) {
      echo[used++] = '\r';
      echo[used++] = '\n';
    }
    write_all(STDOUT_FILENO, echo, used);

    if (quit_length == 0) continue;
    for (ssize_t i = 0; i < count; ++i) {
      if (tail_length == sizeof(tail)) {
        memmove(tail, tail + 1, sizeof(tail) - 1);
        tail_length--;
      }
      tail[tail_length++] = buffer[i];
    }
    if (tail_length >= quit_length &&
        memcmp(tail + tail_length - quit_length, quit, quit_length) == 0) {
      break;
    }
  }
  fprintf(log, "total %llu\n", total);
  fclose(log);
  return 0;
}
