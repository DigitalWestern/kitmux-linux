#!/usr/bin/env bash
set -Eeuo pipefail

if [[ "$(uname -s)" != "Linux" || -z "${DISPLAY:-}" ]]; then
  echo "Run this gate on Linux with DISPLAY set to an existing X11 display." >&2
  exit 1
fi

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "${script_dir}/gate-common.sh"
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/kitmux-phase6-resume.XXXXXX")"
runtime="$temporary_root/runtime"
config="$temporary_root/config"
state="$temporary_root/state"
data="$temporary_root/data"
cache="$temporary_root/cache"
fake_bin="$temporary_root/fake-bin"
socket_path="$temporary_root/kitmux.sock"
state_path="$state/kitmux/state.json"
profiles="$config/kitmux/ssh-profiles.json"
marker="$temporary_root/resume-marker"
marker_two="$temporary_root/resume-marker-two"
ssh_log="$temporary_root/ssh-invocations.log"
log="$temporary_root/app.log"
app_pid=""

cleanup() {
  if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
    kill -KILL "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  fi
  rm -rf -- "$temporary_root" 2>/dev/null || true
}
trap cleanup EXIT

wait_for_log() {
  local pattern="$1"
  for _ in $(seq 1 300); do
    grep -qE "$pattern" "$log" 2>/dev/null && return 0
    if [[ -n "$app_pid" ]] && ! kill -0 "$app_pid" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  cat "$log" >&2 || true
  echo "Kitmux did not report $pattern" >&2
  exit 1
}

build_runtime() {
  KITMUX_BUILD_APP_RUNTIME=1 KITMUX_APP_TEST_HOOKS=ON \
    "$script_dir/build-release-runtime.sh" "$runtime"
}

launch_app() {
  local mode="$1"
  : >"$log"
  env -i \
    DISPLAY="$DISPLAY" \
    HOME="$HOME" \
    LANG=C.UTF-8 \
    PATH="$fake_bin:/usr/bin:/bin" \
    GSK_RENDERER=gl \
    GTK_IM_MODULE=gtk-im-context-simple \
    KITMUX_AUTORESUME="$mode" \
    KITMUX_SOCKET_PATH="$socket_path" \
    KITMUX_SSH_PROFILES_PATH="$profiles" \
    XDG_CONFIG_HOME="$config" \
    XDG_STATE_HOME="$state" \
    XDG_DATA_HOME="$data" \
    XDG_CACHE_HOME="$cache" \
    KITMUX_SSH_INVOCATION_LOG="$ssh_log" \
    "$runtime/bin/kitmux" >"$log" 2>&1 &
  app_pid=$!
  wait_for_log '^kitmux event=control_server_ready '
  wait_for_log '^kitmux event=navigation_ready$'
  wait_for_log '^kitmux event=resume_review rows=2 unchecked=true$'
}

stop_gracefully() {
  kill -TERM "$app_pid"
  wait "$app_pid"
  app_pid=""
}

crash_app() {
  kill -KILL "$app_pid"
  wait "$app_pid" || true
  app_pid=""
}

build_runtime
mkdir -m 700 -p "$config/kitmux" "$state/kitmux" "$data" "$cache" "$fake_bin"
: >"$ssh_log"
chmod 600 "$ssh_log"
cat >"$fake_bin/ssh" <<'SH'
#!/bin/sh
set -eu
printf '%s\n' invoked >>"$KITMUX_SSH_INVOCATION_LOG"
exit 0
SH
chmod 700 "$fake_bin/ssh"

python3 - "$state_path" "$profiles" "$marker" "$marker_two" <<'PY'
import json
import os
import pathlib
import sys

state_path, profiles_path, marker, marker_two = map(pathlib.Path, sys.argv[1:])
workspace = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
pane = "11111111-1111-1111-1111-111111111111"
ssh_pane = "22222222-2222-2222-2222-222222222222"
profile = "33333333-3333-3333-3333-333333333333"

def leaf(pane_id):
    return {"pane": {"_0": {"rawValue": pane_id}}}

state = {
    "version": 1,
    "activeWorkspaceIndex": 0,
    "createdWorkspaceCount": 1,
    "workspaces": [{
        "id": workspace,
        "name": "resume test",
        "activeTabGroupIndex": 0,
        "createdGroupCount": 1,
        "tabGroups": [{
            "name": "main",
            "activeTerminalTabIndex": 0,
            "terminalTabs": [
                {
                    "focusedPaneID": {"rawValue": pane},
                    "root": leaf(pane),
                    "paneDetails": {pane: {
                        "surfaces": [{
                            "id": "44444444-4444-4444-4444-444444444444",
                            "kind": "terminal",
                            "cwd": str(pathlib.Path(marker).parent),
                            "resumeCommand": f"touch {marker}",
                        }, {
                            "id": "55555555-5555-5555-5555-555555555555",
                            "kind": "terminal",
                            "cwd": str(pathlib.Path(marker_two).parent),
                            "resumeCommand": f"touch {marker_two}",
                        }],
                        "activeSurfaceIndex": 1,
                    }},
                },
                {
                    "focusedPaneID": {"rawValue": ssh_pane},
                    "root": leaf(ssh_pane),
                    "paneDetails": {ssh_pane: {"sshProfileID": profile}},
                },
            ],
        }],
    }],
}
state_path.write_text(json.dumps(state, indent=2) + "\n", encoding="utf-8")
os.chmod(state_path, 0o600)
profiles_path.write_text(json.dumps({
    "version": 1,
    "profiles": [{
        "id": profile,
        "name": "restored SSH",
        "hostAlias": "unused",
        "createdAt": "2026-08-18T00:00:00Z",
        "updatedAt": "2026-08-18T00:00:00Z",
    }],
}, indent=2) + "\n", encoding="utf-8")
os.chmod(profiles_path, 0o600)
PY
cp "$state_path" "$temporary_root/original-state.json"
chmod 600 "$temporary_root/original-state.json"

# Review starts unchecked; declining runs nothing and retains both surface rows.
launch_app decline
sleep 0.5
[[ ! -e "$marker" ]]
grep -q '^kitmux event=ssh_agent available=false$' "$log"
grep -q '^kitmux event=child_exit ' "$log"
[[ ! -s "$ssh_log" ]]
stop_gracefully
python3 - "$state_path" "$marker" "$marker_two" <<'PY'
import json
import pathlib
import sys

state = json.loads(pathlib.Path(sys.argv[1]).read_text())
tabs = state["workspaces"][0]["tabGroups"][0]["terminalTabs"]
resume = tabs[0]["paneDetails"]["11111111-1111-1111-1111-111111111111"]
ssh = tabs[1]["paneDetails"]["22222222-2222-2222-2222-222222222222"]
surfaces = resume.get("surfaces") or []
assert len(surfaces) == 2
assert [surface["resumeCommand"] for surface in surfaces] == [
    f"touch {sys.argv[2]}",
    f"touch {sys.argv[3]}",
]
assert ssh["sshProfileID"] == "33333333-3333-3333-3333-333333333333"
PY
[[ -f "$state_path.last-good" ]]

# SIGKILL leaves a stale socket; the next launch replaces it without SSH.
cp "$temporary_root/original-state.json" "$state_path"
launch_app decline
crash_app
[[ -S "$socket_path" ]]
[[ -f "$state_path" && -f "$state_path.last-good" ]]
cp "$temporary_root/original-state.json" "$state_path"
launch_app decline
[[ ! -s "$ssh_log" ]]
stop_gracefully

# Reopen the same v1 state in a fresh runtime. Approval runs only the selected
# command; the restored SSH pane remains a disconnected placeholder.
cp "$temporary_root/original-state.json" "$state_path"
rm -f "$marker"
rm -f "$marker_two"
launch_app restore
for _ in $(seq 1 100); do
  [[ -e "$marker" ]] && break
  sleep 0.1
done
[[ -e "$marker" ]]
[[ ! -e "$marker_two" ]]
[[ ! -s "$ssh_log" ]]
stop_gracefully

# Selecting every displayed surface executes both commands in the same pane.
cp "$temporary_root/original-state.json" "$state_path"
rm -f "$marker" "$marker_two"
launch_app restore-all
for _ in $(seq 1 100); do
  [[ -e "$marker" && -e "$marker_two" ]] && break
  sleep 0.1
done
[[ -e "$marker" && -e "$marker_two" ]]
[[ ! -s "$ssh_log" ]]
stop_gracefully

# A live text mutation after display is rejected by the final identity check.
cp "$temporary_root/original-state.json" "$state_path"
rm -f "$marker"
rm -f "$marker_two"
launch_app race
wait_for_log '^kitmux event=resume_command_skipped reason=identity-changed$'
[[ ! -e "$marker" ]]
stop_gracefully

# Repeated inert launches exercise startup/close churn without accumulating
# live children or SSH invocations.
cp "$temporary_root/original-state.json" "$state_path"
for _ in 1 2 3; do
  launch_app decline
  stop_gracefully
  cp "$temporary_root/original-state.json" "$state_path"
done
[[ ! -s "$ssh_log" ]]

echo "Slice 6.3 resume, recovery, stale-socket, SSH, upgrade, and stress gate: OK"
