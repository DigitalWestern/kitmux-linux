#!/usr/bin/env bash
set -Eeuo pipefail

if [[ "$(uname -s)" != "Linux" || -z "${DISPLAY:-}" ]]; then
  echo "Run this gate on Linux with DISPLAY set to an existing X11 display." >&2
  exit 1
fi
for command in python3 stat realpath grep; do
  command -v "$command" >/dev/null || {
    echo "Missing required command: $command" >&2
    exit 1
  }
done

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "${script_dir}/gate-common.sh"
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/kitmux-phase6-ssh.XXXXXX")"
runtime="$temporary_root/runtime"
config="$temporary_root/config"
state="$temporary_root/state"
data="$temporary_root/data"
cache="$temporary_root/cache"
fake_bin="$temporary_root/nonstandard-ssh-bin"
socket_path="$temporary_root/kitmux.sock"
profiles="$temporary_root/ssh-profiles.json"
argv_log="$temporary_root/ssh-argv.log"
wrapper_used="$temporary_root/ssh-wrapper-used.log"
app_pid=""
log="$temporary_root/app.log"
profile_id="11111111-2222-3333-4444-555555555555"

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
    echo "Slice 6.2 SSH gate failed; current app log:" >&2
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

cli() {
  KITMUX_SOCKET_PATH="$socket_path" "$runtime/bin/kitmuxctl" "$@"
}

build_runtime
mkdir -m 700 "$config" "$state" "$data" "$cache" "$fake_bin"
cp "$script_dir/../tests/ssh-test-executable.sh" "$fake_bin/ssh"
chmod 700 "$fake_bin/ssh"
python3 - "$profiles" "$profile_id" <<'PY'
import json
import os
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
document = {
    "version": 1,
    "profiles": [{
        "id": sys.argv[2],
        "name": "Demo",
        "hostAlias": "demo",
        "remoteCommand": "printf SSH_REMOTE_MARKER",
        "createdAt": "2026-08-17T00:00:00Z",
        "updatedAt": "2026-08-17T00:00:00Z",
    }],
}
path.write_text(json.dumps(document), encoding="utf-8")
os.chmod(path, 0o600)
PY

: >"$wrapper_used"
: >"$argv_log"
chmod 600 "$wrapper_used" "$argv_log"
env -i \
  DISPLAY="$DISPLAY" \
  HOME="$HOME" \
  LANG=C.UTF-8 \
  PATH="$fake_bin:/usr/bin:/bin" \
  GSK_RENDERER=gl \
  GTK_IM_MODULE=gtk-im-context-simple \
  KITMUX_SOCKET_PATH="$socket_path" \
  KITMUX_SSH_PROFILES_PATH="$profiles" \
  KITMUX_SSH_ARGV_LOG="$argv_log" \
  KITMUX_SSH_WRAPPER_USED="$wrapper_used" \
  XDG_CONFIG_HOME="$config" \
  XDG_STATE_HOME="$state" \
  XDG_DATA_HOME="$data" \
  XDG_CACHE_HOME="$cache" \
  "$runtime/bin/kitmux" >"$log" 2>&1 &
app_pid=$!
wait_for_log '^kitmux event=control_server_ready '
wait_for_log '^kitmux event=navigation_ready$'

[[ "$(stat -c "%a" "$profiles")" == "600" ]]
list_json="$temporary_root/profile-list.json"
cli --json ssh profile list >"$list_json"
python3 - "$list_json" "$profile_id" <<'PY'
import json
import pathlib
import sys

result = json.loads(pathlib.Path(sys.argv[1]).read_text())['result']
profile = result['sshProfiles'][0]
assert profile['id'] == sys.argv[2]
assert profile['name'] == 'Demo'
assert profile['hostAlias'] == 'demo'
assert profile['hasRemoteCommand'] is True
PY

review_json="$temporary_root/review.json"
cli --json ssh connect "$profile_id" >"$review_json"
fingerprint="$(python3 - "$review_json" <<'PY'
import json
import pathlib
import sys

result = json.loads(pathlib.Path(sys.argv[1]).read_text())['result']
assert result['connected'] is False
assert result['approvalRequired'] is True
review = result['review']
assert review['hasExternallyListeningForward'] is True
assert review['proxyCommand'] is None
print(review['fingerprint'])
PY
)"
[[ "${#fingerprint}" -eq 64 ]]
printf '%s\n' "$fingerprint" >"$temporary_root/first-fingerprint"

connected_json="$temporary_root/connected.json"
cli --json ssh connect "$profile_id" --approve "$fingerprint" >"$connected_json"
python3 - "$profiles" "$fingerprint" <<'PY'
import json
import pathlib
import sys

profile = json.loads(pathlib.Path(sys.argv[1]).read_text())['profiles'][0]
assert profile['reviewedFingerprint'] == sys.argv[2], profile
PY
pane_id="$(python3 - "$connected_json" <<'PY'
import json
import pathlib
import sys

result = json.loads(pathlib.Path(sys.argv[1]).read_text())['result']
assert result['connected'] is True
assert result['pane']
print(result['pane'])
PY
)"
tree_json="$temporary_root/tree.json"
cli --json tree >"$tree_json"
python3 - "$tree_json" "$pane_id" <<'PY'
import json
import pathlib
import sys

tree = json.loads(pathlib.Path(sys.argv[1]).read_text())['result']
encoded = json.dumps(tree)
assert sys.argv[2] in encoded, sys.argv[2]
PY
wait_for_log '^kitmux event=ssh_agent available=false$'
wait_for_log '^kitmux event=ssh_surface_created '
grep -q '^resolution$' "$wrapper_used"
grep -q '^connect$' "$wrapper_used"

python3 - "$argv_log" <<'PY'
import pathlib
import sys

argv = pathlib.Path(sys.argv[1]).read_text().splitlines()
assert argv == ['--', 'demo', 'printf SSH_REMOTE_MARKER'], argv
PY
if grep -Eq 'SSH_REMOTE_MARKER|proxyCommand|proxycommand|fingerprint' "$log"; then
  echo "expanded SSH arguments or review data leaked into the app log" >&2
  exit 1
fi

reconnect_review_json="$temporary_root/reconnect-review.json"
reconnected_json="$temporary_root/reconnected.json"
reconnect_error="$temporary_root/reconnect-error.log"
reviewed=false
for _ in $(seq 1 50); do
  if cli --json ssh reconnect "$pane_id" >"$reconnect_review_json" 2>"$reconnect_error"; then
    if python3 - "$reconnect_review_json" "$fingerprint" <<'PY'
import json
import pathlib
import sys

result = json.loads(pathlib.Path(sys.argv[1]).read_text())['result']
assert result['connected'] is False
assert result['approvalRequired'] is True
assert result['review']['fingerprint'] == sys.argv[2]
raise SystemExit(0)
PY
    then
      reviewed=true
      break
    fi
  fi
  sleep 0.1
done
if [[ "$reviewed" != true ]]; then
  echo "reconnect review response:" >&2
  cat "$reconnect_review_json" >&2 || true
  echo "reconnect stderr:" >&2
  cat "$reconnect_error" >&2 || true
  echo "first fingerprint:" >&2
  cat "$temporary_root/first-fingerprint" >&2 || true
  echo "profile document:" >&2
  cat "$profiles" >&2 || true
  echo "current app log:" >&2
  cat "$log" >&2 || true
  exit 1
fi
cli --json ssh reconnect "$pane_id" --approve "$fingerprint" >"$reconnected_json"
python3 - "$reconnected_json" <<'PY'
import json
import pathlib
import sys

result = json.loads(pathlib.Path(sys.argv[1]).read_text())['result']
assert result['connected'] is True
assert result['reconnected'] is True
assert result['pane']
PY
wait_for_log '^kitmux event=ssh_reconnected '
python3 - "$argv_log" <<'PY'
import pathlib
import sys

assert pathlib.Path(sys.argv[1]).read_text().splitlines() == [
    '--', 'demo', 'printf SSH_REMOTE_MARKER'
]
PY

echo "Slice 6.2 SSH and agent workflow gate: OK"
