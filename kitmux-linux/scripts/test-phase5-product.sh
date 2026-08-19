#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" || -z "${DISPLAY:-}" ]]; then
  echo "Run this gate on Linux with DISPLAY set to an existing X11 display." >&2
  exit 1
fi
for command in xdotool python3 seq; do
  command -v "${command}" >/dev/null || {
    echo "Missing required command: ${command}" >&2
    exit 1
  }
done

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "${script_dir}/gate-common.sh"
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/kitmux-phase5-product.XXXXXX")"
runtime="${temporary_root}/runtime"
config="${temporary_root}/config"
state="${temporary_root}/state"
data="${temporary_root}/data"
cache="${temporary_root}/cache"
app_pid=""
window_id=""
current_log=""

cleanup() {
  if [[ -n "${app_pid}" ]] && kill -0 "${app_pid}" 2>/dev/null; then
    kill "${app_pid}" 2>/dev/null || true
    wait "${app_pid}" 2>/dev/null || true
  fi
  rm -rf -- "${temporary_root}" 2>/dev/null || true
}
trap cleanup EXIT
dump_failure() {
  local status="$?"
  if [[ -n "${current_log}" && -f "${current_log}" ]]; then
    echo "Phase 5 product gate failed; current log follows:" >&2
    cat "${current_log}" >&2
  fi
  exit "${status}"
}
trap dump_failure ERR

wait_for_log() { # log, regex, description
  local log="$1" pattern="$2" description="$3"
  for _ in $(seq 1 250); do
    grep -qE "${pattern}" "${log}" 2>/dev/null && return 0
    if [[ -n "${app_pid}" ]] && ! kill -0 "${app_pid}" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  echo "Kitmux never reported ${description}; log follows:" >&2
  cat "${log}" >&2
  exit 1
}

launch_app() { # log
  local log="$1"
  current_log="${log}"
  env -i \
    DISPLAY="${DISPLAY}" \
    HOME="${HOME}" \
    LANG="${LANG:-C.UTF-8}" \
    PATH=/usr/bin:/bin \
    GSK_RENDERER=gl \
    GTK_IM_MODULE=gtk-im-context-simple \
    KITMUX_AUTOCLOSE=confirm \
    KITMUX_ACCESSIBILITY_GATE=1 \
    KITMUX_INTERACTION_DIAGNOSTICS=1 \
    XDG_CONFIG_HOME="${config}" \
    XDG_STATE_HOME="${state}" \
    XDG_DATA_HOME="${data}" \
    XDG_CACHE_HOME="${cache}" \
    "${runtime}/bin/kitmux" >"${log}" 2>&1 &
  app_pid=$!
  wait_for_log "${log}" '^kitmux event=terminal_ready pid=[0-9]+' "terminal readiness"
  wait_for_log "${log}" '^kitmux event=navigation_ready$' "navigation readiness"
  wait_for_log "${log}" '^kitmux event=accessibility_ready roles=true focus=true$' \
    "native GTK roles and focus transfer"
  window_id=""
  for _ in $(seq 1 200); do
    window_id="$(xdotool search --onlyvisible --pid "${app_pid}" 2>/dev/null | head -n 1 || true)"
    [[ -n "${window_id}" ]] && break
    sleep 0.1
  done
  [[ -n "${window_id}" ]] || {
    echo "Could not find the Kitmux window." >&2
    cat "${log}" >&2
    exit 1
  }
  xdotool windowactivate --sync "${window_id}"
  xdotool windowfocus --sync "${window_id}"
}

finish_app() { # log, expected sessions, optional window-close
  local log="$1" expected_sessions="$2" mode="${3:-terminal-exit}"
  if [[ "${mode}" == "window-close" ]]; then
    xdotool key --clearmodifiers alt+F4
  else
    xdotool type --clearmodifiers --delay 20 -- exit
    xdotool key --clearmodifiers Return
  fi
  for _ in $(seq 1 250); do
    ! kill -0 "${app_pid}" 2>/dev/null && break
    sleep 0.1
  done
  if kill -0 "${app_pid}" 2>/dev/null || ! wait "${app_pid}"; then
    echo "Kitmux did not exit cleanly; log follows:" >&2
    cat "${log}" >&2
    exit 1
  fi
  grep -qE "^kitmux event=shutdown .* sessions=${expected_sessions} reaped=true$" "${log}" || {
    echo "Kitmux did not reap the expected ${expected_sessions} sessions." >&2
    cat "${log}" >&2
    exit 1
  }
  app_pid=""
}

KITMUX_BUILD_APP_RUNTIME=1 KITMUX_APP_TEST_HOOKS=ON \
  "${script_dir}/build-release-runtime.sh" "${runtime}"
mkdir -p "${config}" "${state}" "${data}" "${cache}"

first_log="${temporary_root}/first.log"
launch_app "${first_log}"

# Open and save settings without a mouse: palette query/activation, native
# focus order, toggle the close review, then reach Save by Tab.
xdotool key --clearmodifiers ctrl+shift+p
wait_for_log "${first_log}" '^kitmux event=command_palette_opened$' "the command palette"
xdotool type --clearmodifiers --delay 20 -- app.settings
xdotool key --clearmodifiers Return
wait_for_log "${first_log}" '^kitmux event=settings_opened$' "settings"
wait_for_log "${first_log}" '^kitmux event=settings_focus control=restore$' "initial settings focus"
xdotool key --clearmodifiers alt+c
wait_for_log "${first_log}" '^kitmux event=settings_focus control=confirm$' "close-review settings focus"
xdotool key --clearmodifiers ctrl+Return
wait_for_log "${first_log}" '^kitmux event=settings_saved$' "keyboard-only settings save"
if ! grep -q '"confirmCloseWithRunningProcess": true' "${config}/kitmux/settings.json"; then
  echo "Settings UI did not persist the close-review toggle." >&2
  cat "${config}/kitmux/settings.json" >&2
  exit 1
fi

# Build a two-workspace, nested, five-session hierarchy and preserve a custom
# title. All actions use the product shortcut/palette paths.
xdotool key --clearmodifiers super+d
wait_for_log "${first_log}" '^kitmux event=split_changed panes=2 ' "the first split"
xdotool key --clearmodifiers super+shift+d
wait_for_log "${first_log}" '^kitmux event=split_changed panes=3 ' "the nested split"
xdotool key --clearmodifiers ctrl+shift+p
xdotool type --clearmodifiers --delay 10 -- workspace.rename
xdotool key --clearmodifiers Return
xdotool type --clearmodifiers --delay 20 -- 'Persisted Workspace'
xdotool key --clearmodifiers Tab Tab Return
wait_for_log "${first_log}" '^kitmux event=navigation_renamed$' "workspace rename"
xdotool key --clearmodifiers super+t
wait_for_log "${first_log}" '^kitmux event=navigation_changed workspaces=1 groups=1 tabs=2 workspace=0 group=0 tab=1$' \
  "the second tab"
xdotool key --clearmodifiers super+n
wait_for_log "${first_log}" '^kitmux event=navigation_changed workspaces=2 groups=1 tabs=1 workspace=1 group=0 tab=0$' \
  "the second workspace"
xdotool key --clearmodifiers super+1
xdotool key --clearmodifiers alt+1
wait_for_log "${first_log}" '^kitmux event=navigation_changed workspaces=2 groups=1 tabs=2 workspace=0 group=0 tab=0$' \
  "the persisted active hierarchy"

xdotool key --clearmodifiers super+d
wait_for_log "${first_log}" '^kitmux event=split_changed panes=4 ' "the foreground-review pane"
xdotool type --clearmodifiers --delay 20 -- 'sleep 60'
xdotool key --clearmodifiers Return
sleep 0.2
xdotool key --clearmodifiers ctrl+shift+p
xdotool type --clearmodifiers --delay 10 -- pane.close
xdotool key --clearmodifiers Return
wait_for_log "${first_log}" '^kitmux event=close_scope_reviewed command=pane.close sessions=1$' \
  "pane foreground-process review"

# A foreground job in a second group must be reviewed when that group closes,
# even though the scope owns more than the originally active session.
xdotool key --clearmodifiers super+alt+t
wait_for_log "${first_log}" '^kitmux event=navigation_changed workspaces=2 groups=2 tabs=1 workspace=0 group=1 tab=0$' \
  "the foreground-review group"
xdotool type --clearmodifiers --delay 20 -- 'sleep 60'
xdotool key --clearmodifiers Return
sleep 0.2
xdotool key --clearmodifiers ctrl+shift+p
xdotool type --clearmodifiers --delay 10 -- group.close
xdotool key --clearmodifiers Return
wait_for_log "${first_log}" '^kitmux event=close_scope_reviewed command=group.close sessions=1$' \
  "scoped foreground-process review"
wait_for_log "${first_log}" '^kitmux event=navigation_changed workspaces=2 groups=1 tabs=2 workspace=0 group=0 tab=0$' \
  "reviewed group close"

xdotool key --clearmodifiers super+n
wait_for_log "${first_log}" '^kitmux event=navigation_changed workspaces=3 groups=1 tabs=1 workspace=2 group=0 tab=0$' \
  "the foreground-review workspace"
xdotool type --clearmodifiers --delay 20 -- 'sleep 60'
xdotool key --clearmodifiers Return
sleep 0.2
xdotool key --clearmodifiers ctrl+shift+p
xdotool type --clearmodifiers --delay 10 -- workspace.close
xdotool key --clearmodifiers Return
wait_for_log "${first_log}" '^kitmux event=close_scope_reviewed command=workspace.close sessions=1$' \
  "workspace foreground-process review"
wait_for_log "${first_log}" '^kitmux event=navigation_changed workspaces=2 groups=1 tabs=1 workspace=1 group=0 tab=0$' \
  "reviewed workspace close"

xdotool type --clearmodifiers --delay 20 -- 'sleep 60'
xdotool key --clearmodifiers Return
sleep 0.2

finish_app "${first_log}" 5 window-close
grep -qE '^kitmux event=close_confirmed foreground_rechecked=true sessions=1$' "${first_log}"
state_file="${state}/kitmux/state.json"
python3 - "${state_file}" <<'PY'
import json, sys
snapshot = json.load(open(sys.argv[1], encoding="utf-8"))
assert snapshot["version"] == 1
assert len(snapshot["workspaces"]) == 2
first = snapshot["workspaces"][0]
assert first["name"] == "Persisted Workspace"
assert len(first["tabGroups"]) == 1
assert len(first["tabGroups"][0]["terminalTabs"]) == 2
nested = first["tabGroups"][0]["terminalTabs"][0]
assert "split" in nested["root"]
assert len(nested["paneDetails"]) == 3
surfaces = []
for workspace in snapshot["workspaces"]:
    for group in workspace["tabGroups"]:
        for tab in group["terminalTabs"]:
            for detail in tab.get("paneDetails", {}).values():
                for surface in detail.get("surfaces", []):
                    assert surface["kind"] == "terminal"
                    assert "resumeCommand" not in surface
                    surfaces.append(surface["id"])
assert len(surfaces) == len(set(surfaces)) == 5
PY
first_state_hash="$(sha256sum "${state_file}" | awk '{print $1}')"

second_log="${temporary_root}/second.log"
launch_app "${second_log}"
wait_for_log "${second_log}" '^kitmux event=persistence_loaded settings=loaded state=loaded cwd=true font=true$' \
  "the saved state"
wait_for_log "${second_log}" '^kitmux event=hierarchy_restored workspaces=2 sessions=5$' \
  "the full saved hierarchy"
if [[ "$(grep -c '^kitmux event=terminal_surface_created ' "${second_log}")" != 4 ]]; then
  echo "Restore did not create the four non-active sessions." >&2
  exit 1
fi
finish_app "${second_log}" 5
second_state_hash="$(sha256sum "${state_file}" | awk '{print $1}')"
if [[ "${first_state_hash}" != "${second_state_hash}" ]]; then
  echo "A restore/save round trip changed the persisted hierarchy." >&2
  diff -u "${state_file}.last-good" "${state_file}" >&2 || true
  exit 1
fi

# Corrupt the primary in this disposable XDG tree. Launch must recover the
# last readable hierarchy before any new state write occurs.
printf '{' >"${state_file}"
third_log="${temporary_root}/third.log"
launch_app "${third_log}"
wait_for_log "${third_log}" '^kitmux event=persistence_loaded settings=loaded state=last-good cwd=true font=true$' \
  "last-good recovery"
wait_for_log "${third_log}" '^kitmux event=hierarchy_restored workspaces=2 sessions=5$' \
  "the recovered hierarchy"
finish_app "${third_log}" 5

echo "Phase 5 product controls, close review, persistence, and accessibility gate: OK"
