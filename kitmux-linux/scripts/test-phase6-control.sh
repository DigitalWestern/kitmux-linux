#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" || -z "${DISPLAY:-}" ]]; then
  echo "Run this gate on Linux with DISPLAY set to an existing X11 display." >&2
  exit 1
fi
for command in python3 stat realpath; do
  command -v "$command" >/dev/null || {
    echo "Missing required command: $command" >&2
    exit 1
  }
done

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/kitmux-phase6-control.XXXXXX")"
runtime="$temporary_root/runtime"
config="$temporary_root/config"
state="$temporary_root/state"
data="$temporary_root/data"
cache="$temporary_root/cache"
socket_path="$temporary_root/kitmux.sock"
app_pid=""
log="$temporary_root/app.log"

cleanup() {
  if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
    kill "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  fi
  rm -rf -- "$temporary_root" 2>/dev/null || true
}
trap cleanup EXIT
dump_failure() {
  local status=$?
  if [[ -f "$log" ]]; then
    echo "Slice 6.1 control gate failed; current app log:" >&2
    cat "$log" >&2
  fi
  exit "$status"
}
trap dump_failure ERR

wait_for_log() {
  local pattern="$1"
  for _ in $(seq 1 250); do
    grep -qE "$pattern" "$log" 2>/dev/null && return 0
    if [[ -n "$app_pid" ]] && ! kill -0 "$app_pid" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  cat "$log" >&2
  echo "Kitmux did not report $pattern" >&2
  exit 1
}

build_runtime() {
  KITMUX_BUILD_APP_RUNTIME=1 KITMUX_APP_TEST_HOOKS=ON \
    "$script_dir/build-release-runtime.sh" "$runtime"
}

launch_app() {
  : >"$log"
  env -i \
    DISPLAY="$DISPLAY" \
    HOME="$HOME" \
    LANG=C.UTF-8 \
    PATH=/usr/bin:/bin \
    GSK_RENDERER=gl \
    GTK_IM_MODULE=gtk-im-context-simple \
    KITMUX_SOCKET_PATH="$socket_path" \
    XDG_CONFIG_HOME="$config" \
    XDG_STATE_HOME="$state" \
    XDG_DATA_HOME="$data" \
    XDG_CACHE_HOME="$cache" \
    "$runtime/bin/kitmux" >"$log" 2>&1 &
  app_pid=$!
  wait_for_log '^kitmux event=control_server_ready '
  wait_for_log '^kitmux event=navigation_ready$'
}

cli() {
  KITMUX_SOCKET_PATH="$socket_path" "$runtime/bin/kitmuxctl" "$@"
}

build_runtime
mkdir -m 700 "$config" "$state" "$data" "$cache"
launch_app

[[ "$(stat -c "%a" "$socket_path")" == "600" ]]
[[ "$(stat -c "%u" "$socket_path")" == "$(id -u)" ]]
[[ "$(stat -c "%F" "$socket_path")" == "socket" ]]
grep -qE '^kitmux event=control_server_ready .* mode=600$' "$log"

cli --json ping | grep -q '"message": "pong"'
cli --json identify | grep -q "\"uid\": $(id -u)"
cli --json tree | grep -q '"workspaces"'
cli workspace new | grep -q '"changed": true'
cli --json tree | grep -q '"createdWorkspaceCount": 2'
cli events --limit 20 | grep -q '"method": "workspace.create"'
cli --json events --limit 20 | grep -q "\"peer_uid\": $(id -u)"

python3 -c 'import socket, sys, time; client = socket.socket(socket.AF_UNIX); client.connect(sys.argv[1]); time.sleep(3)' "$socket_path" &
slow_pid=$!
sleep 0.2
cli ping | grep -q '"message": "pong"'
wait "$slow_pid" || true

python3 - "$socket_path" <<'PY'
import json
import socket
import sys

def send(payload):
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    client.settimeout(3)
    client.connect(sys.argv[1])
    client.sendall(payload)
    result = b""
    while not result.endswith(b"\n"):
        result += client.recv(4096)
    return json.loads(result)

malformed = send(b"{\n")
assert malformed["ok"] is False
assert malformed["error"]["code"] == "malformed_request"
oversized = send((b" " * 65537) + b"\n")
assert oversized["ok"] is False
assert oversized["error"]["code"] == "request_too_large"
PY

kill "$app_pid"
wait "$app_pid" || true
app_pid=""
# ponytail: SIGTERM cannot run Rust destructors; restart must safely replace
# this stale socket, while graceful GTK shutdown remains covered by Phase 5.
[[ -S "$socket_path" ]]

[[ -S "$socket_path" ]]
launch_app
cli ping | grep -q '"message": "pong"'
kill "$app_pid"
wait "$app_pid" || true
app_pid=""
[[ -S "$socket_path" ]]

rm -f "$socket_path"
ln -s "$temporary_root" "$socket_path"
set +e
env -i \
  DISPLAY="$DISPLAY" HOME="$HOME" LANG=C.UTF-8 PATH=/usr/bin:/bin \
  GSK_RENDERER=gl GTK_IM_MODULE=gtk-im-context-simple \
  KITMUX_SOCKET_PATH="$socket_path" \
  XDG_CONFIG_HOME="$config" XDG_STATE_HOME="$state" \
  XDG_DATA_HOME="$data" XDG_CACHE_HOME="$cache" \
  "$runtime/bin/kitmux" >"$temporary_root/symlink.log" 2>&1
symlink_status=$?
set -e
[[ "$symlink_status" -eq 0 ]]
grep -q 'control_server_failed' "$temporary_root/symlink.log"
[[ -L "$socket_path" ]]
rm -f "$socket_path"

echo "Slice 6.1 secure local control and CLI gate: OK"
