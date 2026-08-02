#!/usr/bin/env bash
set -Eeuo pipefail

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
second_pid=""
log="$temporary_root/app.log"

cleanup() {
  if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
    kill "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  fi
  if [[ -n "$second_pid" ]] && kill -0 "$second_pid" 2>/dev/null; then
    kill "$second_pid" 2>/dev/null || true
    wait "$second_pid" 2>/dev/null || true
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

wait_for_log_file() {
  local target_log="$1"
  local target_pid="$2"
  local pattern="$3"
  for _ in $(seq 1 250); do
    grep -qE "$pattern" "$target_log" 2>/dev/null && return 0
    if [[ -n "$target_pid" ]] && ! kill -0 "$target_pid" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  cat "$target_log" >&2
  echo "Kitmux did not report $pattern" >&2
  exit 1
}

wait_for_log() {
  wait_for_log_file "$log" "$app_pid" "$1"
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

cli --json ping | grep '"message": "pong"' >/dev/null
cli --json identify | grep "\"uid\": $(id -u)" >/dev/null
cli --json tree | grep '"workspaces"' >/dev/null
cli workspace new | grep '"changed": true' >/dev/null
cli --json tree | grep '"createdWorkspaceCount": 2' >/dev/null
cli events --limit 20 | grep '"method": "workspace.create"' >/dev/null
cli --json events --limit 20 | grep "\"peer_uid\": $(id -u)" >/dev/null

# The Rust socket tests own the client-cap and total-deadline assertions;
# this GUI gate keeps only one idle-client responsiveness check.
python3 -c 'import socket, sys, time; client = socket.socket(socket.AF_UNIX); client.connect(sys.argv[1]); time.sleep(3)' "$socket_path" &
slow_pid=$!
sleep 0.2
cli ping | grep '"message": "pong"' >/dev/null
wait "$slow_pid" || true

python3 - "$socket_path" <<'PY'
import json
import socket
import sys

def send(payload):
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    client.settimeout(3)
    client.connect(sys.argv[1])
    try:
        for offset in range(0, len(payload), 4096):
            client.sendall(payload[offset:offset + 4096])
    except BrokenPipeError:
        pass
    result = b""
    while not result.endswith(b"\n"):
        chunk = client.recv(4096)
        assert chunk, "server closed without an error response"
        result += chunk
    return json.loads(result)

malformed = send(b"{\n")
assert malformed["ok"] is False
assert malformed["error"]["code"] == "malformed_request"
oversized = send((b" " * 65537) + b"\n")
assert oversized["ok"] is False
assert oversized["error"]["code"] == "request_too_large"
PY

pane_split="$(cli pane split right)"
echo "$pane_split" | grep -q '"changed": true'
pane_ids="$(cli --json tree | python3 -c '
import json, sys
tree = json.load(sys.stdin).get("result", {})
ids = []
for workspace in tree["workspaces"]:
    for group in workspace["tabGroups"]:
        for tab in group["terminalTabs"]:
            ids.extend((tab.get("paneDetails") or {}).keys())
assert len(ids) >= 2, ids
print(" ".join(ids[:2]))
')"
read -r pane_a pane_b <<<"$pane_ids"
cli pane focus "$pane_a" | grep '"changed": true' >/dev/null
cli pane send "$pane_b" "KITMUX_PANE_B_MARKER" | grep '"byteCount"' >/dev/null
sleep 0.5
cli pane focus "$pane_a" | grep '"changed": true' >/dev/null
navigation_events_before="$(grep -c '^kitmux event=navigation_changed ' "$log" || true)"
cli --json pane read-screen "$pane_b" >"$temporary_root/pane-b-screen.json"
if ! grep -q 'KITMUX_PANE_B_' "$temporary_root/pane-b-screen.json"; then
  echo "pane B screen response:" >&2
  cat "$temporary_root/pane-b-screen.json" >&2
  exit 1
fi
cli --json pane read-screen "$pane_a" >"$temporary_root/pane-a-screen.json"
if grep -q 'KITMUX_PANE_B_' "$temporary_root/pane-a-screen.json"; then
  echo "pane A unexpectedly received pane B text" >&2
  exit 1
fi
navigation_events_after="$(grep -c '^kitmux event=navigation_changed ' "$log" || true)"
[[ "$navigation_events_after" -eq "$navigation_events_before" ]]
cli --json pane read-screen current >"$temporary_root/current-screen.json"
if grep -q 'KITMUX_PANE_B_' "$temporary_root/current-screen.json"; then
  echo "read-only pane targeting changed the focused pane" >&2
  exit 1
fi

second_log="$temporary_root/second.log"
env -i \
  DISPLAY="$DISPLAY" HOME="$HOME" LANG=C.UTF-8 PATH=/usr/bin:/bin \
  GSK_RENDERER=gl GTK_IM_MODULE=gtk-im-context-simple \
  KITMUX_SOCKET_PATH="$socket_path" \
  XDG_CONFIG_HOME="$config" XDG_STATE_HOME="$state" \
  XDG_DATA_HOME="$data" XDG_CACHE_HOME="$cache" \
  "$runtime/bin/kitmux" >"$second_log" 2>&1 &
second_pid=$!
wait_for_log_file "$second_log" "$second_pid" '^kitmux event=control_server_declined reason=live_server$'
wait_for_log_file "$second_log" "$second_pid" '^kitmux event=navigation_ready$'
cli ping | grep '"message": "pong"' >/dev/null
kill "$app_pid"
wait "$app_pid" || true
app_pid=""
if grep -q '^kitmux event=control_server_ready ' "$second_log"; then
  echo "second instance unexpectedly acquired the control socket" >&2
  exit 1
fi
kill "$second_pid"
wait "$second_pid" || true
second_pid=""
[[ -S "$socket_path" ]]

# ponytail: SIGTERM cannot run Rust destructors; restart must safely replace
# this stale socket, while graceful GTK shutdown remains covered by Phase 5.
launch_app
cli ping | grep -q '"message": "pong"'

default_runtime="$temporary_root/xdg-runtime"
mkdir -m 700 "$default_runtime"
default_log="$temporary_root/default.log"
env -i \
  DISPLAY="$DISPLAY" HOME="$HOME" LANG=C.UTF-8 PATH=/usr/bin:/bin \
  GSK_RENDERER=gl GTK_IM_MODULE=gtk-im-context-simple \
  XDG_RUNTIME_DIR="$default_runtime" \
  XDG_CONFIG_HOME="$config" XDG_STATE_HOME="$state" \
  XDG_DATA_HOME="$data" XDG_CACHE_HOME="$cache" \
  "$runtime/bin/kitmux" >"$default_log" 2>&1 &
default_pid=$!
wait_for_log_file "$default_log" "$default_pid" '^kitmux event=control_server_ready '
wait_for_log_file "$default_log" "$default_pid" '^kitmux event=navigation_ready$'
default_socket="$default_runtime/kitmux/kitmux.sock"
[[ -S "$default_socket" ]]
KITMUX_SOCKET_PATH="$default_socket" "$runtime/bin/kitmuxctl" ping | grep '"message": "pong"' >/dev/null
kill "$default_pid"
wait "$default_pid" || true

user_bin="$temporary_root/user-bin"
XDG_BIN_HOME="$user_bin" HOME="$HOME" \
  "$script_dir/install-user-cli.sh" "$runtime/bin/kitmuxctl" >"$temporary_root/install-user-cli.log"
PATH="$user_bin:/usr/bin:/bin" KITMUX_SOCKET_PATH="$socket_path" \
  kitmuxctl ping | grep '"message": "pong"' >/dev/null
rm -f "$runtime/bin/kitmuxctl"
if XDG_BIN_HOME="$user_bin" HOME="$HOME" \
  "$script_dir/install-user-cli.sh" "$user_bin/kitmuxctl" \
  >"$temporary_root/install-user-cli-missing.log" 2>&1; then
  install_missing_status=0
else
  install_missing_status=$?
fi
[[ "$install_missing_status" -eq 2 ]]
grep -q 'source must be an executable' "$temporary_root/install-user-cli-missing.log"

kill "$app_pid"
wait "$app_pid" || true
app_pid=""
[[ -S "$socket_path" ]]

rm -f "$socket_path"
ln -s "$temporary_root" "$socket_path"
env -i \
  DISPLAY="$DISPLAY" HOME="$HOME" LANG=C.UTF-8 PATH=/usr/bin:/bin \
  GSK_RENDERER=gl GTK_IM_MODULE=gtk-im-context-simple \
  KITMUX_SOCKET_PATH="$socket_path" \
  XDG_CONFIG_HOME="$config" XDG_STATE_HOME="$state" \
  XDG_DATA_HOME="$data" XDG_CACHE_HOME="$cache" \
  "$runtime/bin/kitmux" >"$temporary_root/symlink.log" 2>&1 &
symlink_pid=$!
wait_for_log_file "$temporary_root/symlink.log" "$symlink_pid" '^kitmux event=control_server_failed '
wait_for_log_file "$temporary_root/symlink.log" "$symlink_pid" '^kitmux event=navigation_ready$'
kill "$symlink_pid"
wait "$symlink_pid" || true
grep -q 'control_server_failed' "$temporary_root/symlink.log"
[[ -L "$socket_path" ]]
! grep -q 'was not found' "$log"
rm -f "$socket_path"

echo "Slice 6.1 secure local control and CLI gate: OK"
