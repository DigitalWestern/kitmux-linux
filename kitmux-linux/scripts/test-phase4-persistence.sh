#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" || -z "${DISPLAY:-}" ]]; then
  echo "Run this gate on Linux with DISPLAY set." >&2
  exit 1
fi
for command in dd find install mount mountpoint python3 sha256sum stat sudo umount xdotool xsel; do
  command -v "${command}" >/dev/null || {
    echo "Missing required command: ${command}" >&2
    exit 1
  }
done
sudo -n true 2>/dev/null || {
  echo "The ENOSPC gate needs non-interactive sudo for one private tmpfs mount." >&2
  exit 1
}

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/kitmux-phase4-persistence.XXXXXX")"
runtime="${temporary_root}/runtime"
restore_cwd="${temporary_root}/restored-cwd"
resume_marker="${temporary_root}/resume-command-ran"
full_disk_mount="${temporary_root}/full-disk"
app_pid=""
child_pid=""

cleanup() {
  if [[ -n "${app_pid}" ]] && kill -0 "${app_pid}" 2>/dev/null; then
    kill "${app_pid}" 2>/dev/null || true
    wait "${app_pid}" 2>/dev/null || true
  fi
  if [[ -n "${child_pid}" ]] && kill -0 "${child_pid}" 2>/dev/null; then
    kill -KILL "${child_pid}" 2>/dev/null || true
  fi
  if mountpoint -q "${full_disk_mount}" 2>/dev/null; then
    sudo -n umount "${full_disk_mount}" 2>/dev/null || true
  fi
  rm -rf -- "${temporary_root}"
}
trap cleanup EXIT

wait_for_log() { # log, regex, description
  local log="$1" pattern="$2" description="$3"
  for _ in $(seq 1 200); do
    grep -qE "${pattern}" "${log}" 2>/dev/null && return 0
    if [[ -n "${app_pid}" ]] && ! kill -0 "${app_pid}" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  echo "Kitmux never reported ${description}." >&2
  cat "${log}" >&2
  exit 1
}

launch_app() { # root, log, optional state root
  local root="$1" log="$2" state_root="${3:-${1}/state}"
  install -d -m 700 "${root}" "${root}/home" "${root}/config" \
    "${state_root}" "${root}/data" "${root}/cache"
  env -i \
    DISPLAY="${DISPLAY}" HOME="${HOME}" LANG="${LANG:-C.UTF-8}" \
    PATH=/usr/bin:/bin GTK_IM_MODULE=gtk-im-context-simple GSK_RENDERER=gl \
    XDG_CONFIG_HOME="${root}/config" XDG_STATE_HOME="${state_root}" \
    XDG_DATA_HOME="${root}/data" XDG_CACHE_HOME="${root}/cache" \
    KITMUX_AUTOPASTE=cancel KITMUX_INTERACTION_DIAGNOSTICS=1 \
    "${runtime}/bin/kitmux" >"${log}" 2>&1 &
  app_pid=$!
  wait_for_log "${log}" '^kitmux event=terminal_ready pid=[0-9]+' "terminal readiness"
  child_pid="$(sed -n 's/^kitmux event=terminal_ready pid=\([0-9][0-9]*\).*/\1/p' "${log}" | head -n 1)"
  kill -0 "${child_pid}" 2>/dev/null
}

window_for_app() {
  local window_id=""
  for _ in $(seq 1 200); do
    window_id="$(xdotool search --onlyvisible --pid "${app_pid}" 2>/dev/null | head -n 1 || true)"
    [[ -n "${window_id}" ]] && break
    sleep 0.1
  done
  [[ -n "${window_id}" ]] || {
    echo "Could not find the Kitmux persistence-test window." >&2
    exit 1
  }
  printf '%s\n' "${window_id}"
}

stop_shell() { # log
  local log="$1" stopped_app="${app_pid}" stopped_child="${child_pid}"
  kill -HUP "${stopped_child}"
  for _ in $(seq 1 200); do
    ! kill -0 "${stopped_app}" 2>/dev/null && break
    sleep 0.1
  done
  if kill -0 "${stopped_app}" 2>/dev/null; then
    echo "Kitmux did not close after its shell exited." >&2
    cat "${log}" >&2
    exit 1
  fi
  wait "${stopped_app}"
  app_pid=""
  if kill -0 "${stopped_child}" 2>/dev/null; then
    echo "Kitmux left shell child ${stopped_child} alive." >&2
    exit 1
  fi
  child_pid=""
}

KITMUX_BUILD_APP_RUNTIME=1 "${script_dir}/build-release-runtime.sh" "${runtime}"
mkdir -p "${restore_cwd}"

# First launch: empty XDG roots use defaults and create private state only on
# clean shutdown after cwd/font changes.
roundtrip_root="${temporary_root}/roundtrip"
first_log="${temporary_root}/first.log"
launch_app "${roundtrip_root}" "${first_log}"
wait_for_log "${first_log}" \
  '^kitmux event=persistence_loaded settings=missing state=missing cwd=false font=false$' \
  "missing-file defaults"
first_pid="${child_pid}"
window_id="$(window_for_app)"
xdotool windowactivate --sync "${window_id}"
xdotool click --window "${window_id}" 1
xdotool type --clearmodifiers --delay 1 -- "cd \"${restore_cwd}\""
xdotool key --clearmodifiers Return
wait_for_log "${first_log}" '^kitmux event=cwd_updated valid=true$' "saved cwd"
xdotool key --clearmodifiers ctrl+shift+equal
wait_for_log "${first_log}" '^kitmux event=font_size points=' "saved font"
stop_shell "${first_log}"

state_path="${roundtrip_root}/state/kitmux/state.json"
settings_path="${roundtrip_root}/config/kitmux/settings.json"
[[ -f "${state_path}" && -f "${state_path}.last-good" ]]
[[ "$(stat -c '%a' "${state_path}")" == 600 ]]
[[ ! -e "${settings_path}" ]]

# Resume text is deliberately seeded as inert data. The second launch may use
# cwd/font and stable IDs, but must create a different passwd shell and never
# execute this command.
python3 - "${state_path}" "${resume_marker}" <<'PY'
import json
import pathlib
import sys

path, marker = map(pathlib.Path, sys.argv[1:])
state = json.loads(path.read_text())
detail = next(iter(state["workspaces"][0]["tabGroups"][0]["terminalTabs"][0]["paneDetails"].values()))
detail["resumeCommand"] = f"touch {marker}"
path.write_text(json.dumps(state, indent=2) + "\n")
path.chmod(0o600)
PY
state_ids="$(python3 - "${state_path}" <<'PY'
import json, pathlib, sys
state = json.loads(pathlib.Path(sys.argv[1]).read_text())
tab = state["workspaces"][0]["tabGroups"][0]["terminalTabs"][0]
print(state["workspaces"][0]["id"], tab["focusedPaneID"])
PY
)"

second_log="${temporary_root}/second.log"
launch_app "${roundtrip_root}" "${second_log}"
wait_for_log "${second_log}" \
  '^kitmux event=persistence_loaded settings=missing state=loaded cwd=true font=true$' \
  "state restore"
wait_for_log "${second_log}" '^kitmux event=font_restored points=' "font restore"
[[ "${child_pid}" != "${first_pid}" ]]
[[ "$(realpath "/proc/${child_pid}/cwd")" == "$(realpath "${restore_cwd}")" ]]
[[ ! -e "${resume_marker}" ]]

# The polling watcher is armed before the config directory exists and detects
# an editor-style atomic replacement. A one-byte threshold then makes the
# next clipboard paste take the unsafe/cancel path.
install -d -m 700 "$(dirname "${settings_path}")"
printf '%s\n' \
  '{"version":1,"pasteConfirmationThresholdBytes":1,"confirmCloseWithRunningProcess":true,"unknown":"kept"}' \
  >"${settings_path}.new"
chmod 600 "${settings_path}.new"
mv "${settings_path}.new" "${settings_path}"
wait_for_log "${second_log}" \
  '^kitmux event=settings_reloaded paste_threshold=1 confirm_close=true$' \
  "atomic settings replacement"
printf '%s' "printf watcher-paste-ran > '${temporary_root}/watcher-paste-ran'" \
  | xsel --clipboard --input
window_id="$(window_for_app)"
xdotool windowactivate --sync "${window_id}"
xdotool key --clearmodifiers ctrl+shift+v
wait_for_log "${second_log}" '^kitmux event=paste_cancelled reason=large$' \
  "reloaded paste threshold"
[[ ! -e "${temporary_root}/watcher-paste-ran" ]]

# A malformed replacement is ignored without quarantine or an in-memory
# reset. Removal followed by a valid atomic recreation recovers, and replacing
# a file with identical content is suppressed by content equality.
reload_count="$(grep -c '^kitmux event=settings_reloaded ' "${second_log}" || true)"
printf '{' >"${settings_path}.new"
chmod 600 "${settings_path}.new"
mv "${settings_path}.new" "${settings_path}"
sleep 0.6
[[ "$(grep -c '^kitmux event=settings_reloaded ' "${second_log}" || true)" == "${reload_count}" ]]
cancel_count="$(grep -c '^kitmux event=paste_cancelled reason=large$' "${second_log}" || true)"
xdotool key --clearmodifiers ctrl+shift+v
for _ in $(seq 1 100); do
  [[ "$(grep -c '^kitmux event=paste_cancelled reason=large$' "${second_log}" || true)" -gt "${cancel_count}" ]] && break
  sleep 0.1
done
[[ "$(grep -c '^kitmux event=paste_cancelled reason=large$' "${second_log}" || true)" -gt "${cancel_count}" ]]

rm "${settings_path}"
sleep 0.6
printf '%s\n' \
  '{"version":1,"pasteConfirmationThresholdBytes":10000,"confirmCloseWithRunningProcess":true,"unknown":"kept"}' \
  >"${settings_path}.new"
chmod 600 "${settings_path}.new"
mv "${settings_path}.new" "${settings_path}"
wait_for_log "${second_log}" \
  '^kitmux event=settings_reloaded paste_threshold=10000 confirm_close=true$' \
  "valid settings recreation"
reload_count="$(grep -c '^kitmux event=settings_reloaded ' "${second_log}" || true)"
cp "${settings_path}" "${settings_path}.new"
mv "${settings_path}.new" "${settings_path}"
sleep 0.6
[[ "$(grep -c '^kitmux event=settings_reloaded ' "${second_log}" || true)" == "${reload_count}" ]]

recovered_paste_marker="${temporary_root}/watcher-recovered-paste-ran"
printf '%s' "printf watcher-recovered > '${recovered_paste_marker}'" \
  | xsel --clipboard --input
xdotool key --clearmodifiers ctrl+shift+v
xdotool key --clearmodifiers Return
for _ in $(seq 1 100); do
  [[ -e "${recovered_paste_marker}" ]] && break
  sleep 0.1
done
[[ -e "${recovered_paste_marker}" ]]
stop_shell "${second_log}"
[[ ! -e "${resume_marker}" ]]
[[ "${state_ids}" == "$(python3 - "${state_path}" <<'PY'
import json, pathlib, sys
state = json.loads(pathlib.Path(sys.argv[1]).read_text())
tab = state["workspaces"][0]["tabGroups"][0]["terminalTabs"][0]
print(state["workspaces"][0]["id"], tab["focusedPaneID"])
PY
)" ]]

# The restored cwd is seeded before PTY output arrives, so an immediate shell
# exit cannot overwrite it with account home while waiting for OSC 7.
immediate_root="${temporary_root}/immediate-close"
install -d -m 700 "${immediate_root}/state/kitmux"
cp "${state_path}" "${immediate_root}/state/kitmux/state.json"
chmod 600 "${immediate_root}/state/kitmux/state.json"
immediate_log="${temporary_root}/immediate.log"
launch_app "${immediate_root}" "${immediate_log}"
wait_for_log "${immediate_log}" '^kitmux event=cwd_restore_seeded$' \
  "pre-output cwd seed"
stop_shell "${immediate_log}"
python3 - "${immediate_root}/state/kitmux/state.json" "${restore_cwd}" <<'PY'
import json
import pathlib
import sys

state = json.loads(pathlib.Path(sys.argv[1]).read_text())
expected = pathlib.Path(sys.argv[2])
detail = next(iter(state["workspaces"][0]["tabGroups"][0]["terminalTabs"][0]["paneDetails"].values()))
assert pathlib.Path(detail["cwd"]) == expected
PY

# An otherwise valid snapshot with a vanished cwd must still restore its safe
# fields while launching the shell in the account home directory.
python3 - "${state_path}" "${temporary_root}/vanished-cwd" <<'PY'
import json
import pathlib
import sys

path, missing_cwd = map(pathlib.Path, sys.argv[1:])
state = json.loads(path.read_text())
detail = next(iter(state["workspaces"][0]["tabGroups"][0]["terminalTabs"][0]["paneDetails"].values()))
detail["cwd"] = str(missing_cwd)
path.write_text(json.dumps(state, indent=2) + "\n")
path.chmod(0o600)
PY
invalid_cwd_log="${temporary_root}/invalid-cwd.log"
launch_app "${roundtrip_root}" "${invalid_cwd_log}"
wait_for_log "${invalid_cwd_log}" \
  '^kitmux event=persistence_loaded settings=loaded state=loaded cwd=false font=true$' \
  "invalid cwd fallback"
[[ "$(realpath "/proc/${child_pid}/cwd")" == "$(realpath "${HOME}")" ]]
stop_shell "${invalid_cwd_log}"

run_set_aside_case() { # label, settings bytes, state bytes, suffix regex
  local label="$1" settings_bytes="$2" state_bytes="$3" suffix="$4"
  local root="${temporary_root}/${label}" log="${temporary_root}/${label}.log"
  install -d -m 700 "${root}/config/kitmux" "${root}/state/kitmux"
  printf '%s' "${settings_bytes}" >"${root}/config/kitmux/settings.json"
  printf '%s' "${state_bytes}" >"${root}/state/kitmux/state.json"
  chmod 600 "${root}/config/kitmux/settings.json" "${root}/state/kitmux/state.json"
  launch_app "${root}" "${log}"
  wait_for_log "${log}" \
    '^kitmux event=persistence_loaded settings=set-aside state=set-aside cwd=false font=false$' \
    "${label} set-aside policy"
  stop_shell "${log}"
  settings_aside="$(find "${root}/config/kitmux" -maxdepth 1 -type f -name "settings.json.${suffix}*" -print -quit)"
  state_aside="$(find "${root}/state/kitmux" -maxdepth 1 -type f -name "state.json.${suffix}*" -print -quit)"
  [[ -n "${settings_aside}" && -n "${state_aside}" ]]
  [[ "$(<"${settings_aside}")" == "${settings_bytes}" ]]
  [[ "$(<"${state_aside}")" == "${state_bytes}" ]]
}

run_set_aside_case corrupt 'not-settings-json' 'not-state-json' 'corrupt-'
run_set_aside_case newer '{"version":99,"sentinel":"settings"}' \
  '{"version":99,"sentinel":"state"}' 'v99-backup-'

# A valid file in a read-only app directory remains loadable; the failed save
# leaves it byte-identical. A corrupt file that cannot be quarantined is kept
# byte-identical and disables state saving for that launch.
readonly_root="${temporary_root}/readonly"
install -d -m 700 "${readonly_root}/config/kitmux" "${readonly_root}/state/kitmux"
cp "${state_path}" "${readonly_root}/state/kitmux/state.json"
printf '%s\n' '{"version":1}' >"${readonly_root}/config/kitmux/settings.json"
chmod 600 "${readonly_root}"/*/kitmux/*
readonly_hash="$(sha256sum "${readonly_root}/state/kitmux/state.json" | cut -d' ' -f1)"
chmod 500 "${readonly_root}/config/kitmux" "${readonly_root}/state/kitmux"
readonly_log="${temporary_root}/readonly.log"
launch_app "${readonly_root}" "${readonly_log}"
stop_shell "${readonly_log}"
wait_for_log "${readonly_log}" '^kitmux event=state_save_failed$' "read-only save failure"
[[ "${readonly_hash}" == "$(sha256sum "${readonly_root}/state/kitmux/state.json" | cut -d' ' -f1)" ]]
chmod 700 "${readonly_root}/config/kitmux" "${readonly_root}/state/kitmux"

blocked_root="${temporary_root}/blocked-quarantine"
install -d -m 700 "${blocked_root}/config/kitmux" "${blocked_root}/state/kitmux"
printf bad >"${blocked_root}/config/kitmux/settings.json"
printf bad >"${blocked_root}/state/kitmux/state.json"
chmod 600 "${blocked_root}"/*/kitmux/*
blocked_hash="$(sha256sum "${blocked_root}/state/kitmux/state.json" | cut -d' ' -f1)"
chmod 500 "${blocked_root}/config/kitmux" "${blocked_root}/state/kitmux"
blocked_log="${temporary_root}/blocked.log"
launch_app "${blocked_root}" "${blocked_log}"
wait_for_log "${blocked_log}" \
  '^kitmux event=persistence_loaded settings=unreadable state=unreadable cwd=false font=false$' \
  "blocked quarantine preservation"
stop_shell "${blocked_log}"
wait_for_log "${blocked_log}" '^kitmux event=state_save_skipped reason=unsafe-input$' \
  "blocked state overwrite"
[[ "${blocked_hash}" == "$(sha256sum "${blocked_root}/state/kitmux/state.json" | cut -d' ' -f1)" ]]
chmod 700 "${blocked_root}/config/kitmux" "${blocked_root}/state/kitmux"

# Real ENOSPC: seed a readable state in a private 64 KiB tmpfs, fill the
# remaining blocks, and prove the failed atomic save preserves the old hash
# and leaves no temporary file.
install -d -m 700 "${full_disk_mount}"
sudo -n mount -t tmpfs -o "size=64k,mode=700,uid=$(id -u),gid=$(id -g)" \
  tmpfs "${full_disk_mount}"
install -d -m 700 "${full_disk_mount}/state/kitmux"
cp "${state_path}" "${full_disk_mount}/state/kitmux/state.json"
chmod 600 "${full_disk_mount}/state/kitmux/state.json"
full_hash="$(sha256sum "${full_disk_mount}/state/kitmux/state.json" | cut -d' ' -f1)"
dd if=/dev/zero of="${full_disk_mount}/filler" bs=1024 2>/dev/null || true
full_root="${temporary_root}/full-root"
full_log="${temporary_root}/full.log"
launch_app "${full_root}" "${full_log}" "${full_disk_mount}/state"
stop_shell "${full_log}"
wait_for_log "${full_log}" '^kitmux event=state_save_failed$' "ENOSPC save failure"
[[ "${full_hash}" == "$(sha256sum "${full_disk_mount}/state/kitmux/state.json" | cut -d' ' -f1)" ]]
if find "${full_disk_mount}/state/kitmux" -maxdepth 1 -name '.kitmux-write-*' | grep -q .; then
  echo "ENOSPC left a partial persistence temporary file." >&2
  exit 1
fi
sudo -n umount "${full_disk_mount}"

echo "Phase 4 crash-safe persistence gate: OK"
