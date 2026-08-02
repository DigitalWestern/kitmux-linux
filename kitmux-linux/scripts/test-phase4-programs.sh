#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" || -z "${DISPLAY:-}" ]]; then
  echo "Run this gate on Linux with DISPLAY set." >&2
  exit 1
fi
for command in bash fish less tmux vim xdotool zsh; do
  command -v "${command}" >/dev/null || {
    echo "Missing required program: ${command}" >&2
    exit 1
  }
done

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/kitmux-phase4-programs.XXXXXX")"
runtime="${temporary_root}/runtime"
log="${temporary_root}/kitmux.log"
app_pid=""
child_pid=""

cleanup() {
  TMUX_TMPDIR="${temporary_root}/tmux" tmux -L phase4-programs kill-server \
    >/dev/null 2>&1 || true
  if [[ -n "${app_pid}" ]] && kill -0 "${app_pid}" 2>/dev/null; then
    kill "${app_pid}" 2>/dev/null || true
    wait "${app_pid}" 2>/dev/null || true
  fi
  [[ -n "${child_pid}" ]] && kill -KILL "${child_pid}" 2>/dev/null || true
  rm -rf -- "${temporary_root}"
}
trap cleanup EXIT

wait_for_log() { # regex, description
  local pattern="$1" description="$2"
  for _ in $(seq 1 200); do
    grep -qE "${pattern}" "${log}" 2>/dev/null && return 0
    sleep 0.1
  done
  echo "Kitmux never reported ${description}." >&2
  cat "${log}" >&2
  exit 1
}

wait_for_file() { # path, description
  local path="$1" description="$2"
  for _ in $(seq 1 200); do
    [[ -e "${path}" ]] && return 0
    sleep 0.1
  done
  echo "Timed out waiting for ${description}." >&2
  cat "${log}" >&2
  exit 1
}

type_command() {
  xdotool type --clearmodifiers --delay 1 -- "$1"
  xdotool key --clearmodifiers Return
}

KITMUX_BUILD_APP_RUNTIME=1 KITMUX_APP_TEST_HOOKS=ON \
  "${script_dir}/build-release-runtime.sh" "${runtime}"
install -d -m 700 "${temporary_root}/home" "${temporary_root}/config" \
  "${temporary_root}/state" "${temporary_root}/data" "${temporary_root}/cache" \
  "${temporary_root}/tmux"
env -i DISPLAY="${DISPLAY}" HOME="${HOME}" LANG="${LANG:-C.UTF-8}" \
  PATH=/usr/bin:/bin GTK_IM_MODULE=gtk-im-context-simple GSK_RENDERER=gl \
  XDG_CONFIG_HOME="${temporary_root}/config" \
  XDG_STATE_HOME="${temporary_root}/state" \
  XDG_DATA_HOME="${temporary_root}/data" \
  XDG_CACHE_HOME="${temporary_root}/cache" \
  "${runtime}/bin/kitmux" >"${log}" 2>&1 &
app_pid=$!
wait_for_log '^kitmux event=terminal_ready pid=[0-9]+' "terminal readiness"
child_pid="$(sed -n 's/^kitmux event=terminal_ready pid=\([0-9][0-9]*\).*/\1/p' "${log}" | head -n 1)"

window_id=""
for _ in $(seq 1 200); do
  window_id="$(xdotool search --onlyvisible --pid "${app_pid}" 2>/dev/null | head -n 1 || true)"
  [[ -n "${window_id}" ]] && break
  sleep 0.1
done
[[ -n "${window_id}" ]]
xdotool windowactivate --sync "${window_id}"
xdotool click --window "${window_id}" 1

bash_marker="${temporary_root}/bash-ran"
bash_return_marker="${temporary_root}/bash-returned"
type_command "bash --noprofile --norc; printf bash-returned > '${bash_return_marker}'"
sleep 0.3
type_command "printf bash-ok > '${bash_marker}'; exit"
wait_for_file "${bash_marker}" "bash command"
wait_for_file "${bash_return_marker}" "return from bash"

zsh_marker="${temporary_root}/zsh-ran"
zsh_return_marker="${temporary_root}/zsh-returned"
type_command "zsh -f; printf zsh-returned > '${zsh_return_marker}'"
sleep 0.3
type_command "printf zsh-ok > '${zsh_marker}'; exit"
wait_for_file "${zsh_marker}" "zsh command"
wait_for_file "${zsh_return_marker}" "return from zsh"

fish_marker="${temporary_root}/fish-ran"
fish_return_marker="${temporary_root}/fish-returned"
fish_ready_marker="${temporary_root}/fish-ready"
type_command "fish --no-config --init-command=\"printf fish-ready > '${fish_ready_marker}'\"; printf fish-returned > '${fish_return_marker}'"
wait_for_file "${fish_ready_marker}" "fish readiness"
xdotool key --clearmodifiers ctrl+c
sleep 0.2
type_command "touch '${fish_marker}'"
wait_for_file "${fish_marker}" "fish command"
type_command exit
wait_for_file "${fish_return_marker}" "return from fish"

vim_file="${temporary_root}/vim-input.txt"
vim_ready="${temporary_root}/vim-ready"
vim_returned="${temporary_root}/vim-returned"
type_command "vim -u NONE -N -c \"call writefile(['ready'], '${vim_ready}')\" '${vim_file}'; printf vim-returned > '${vim_returned}'"
wait_for_file "${vim_ready}" "vim readiness"
sleep 1
xdotool key --clearmodifiers Escape g g d G
xdotool key --clearmodifiers i
xdotool type --clearmodifiers --delay 20 -- abc
xdotool key --clearmodifiers Left
xdotool type --clearmodifiers --delay 20 -- X
xdotool key --clearmodifiers Escape
xdotool type --clearmodifiers --delay 20 -- :wq
xdotool key --clearmodifiers Return
wait_for_file "${vim_file}" "vim write"
[[ "$(<"${vim_file}")" == abXc ]]
wait_for_file "${vim_returned}" "return from vim"

less_file="${temporary_root}/less-input.txt"
seq 1 500 >"${less_file}"
less_returned="${temporary_root}/less-returned"
type_command "less -X '${less_file}'; printf less-returned > '${less_returned}'"
sleep 0.5
xdotool key --clearmodifiers Page_Down Up q
less_marker="${temporary_root}/less-ran"
type_command "printf less-ok > '${less_marker}'"
wait_for_file "${less_marker}" "less return"
wait_for_file "${less_returned}" "parent-shell return from less"

tmux_marker="${temporary_root}/tmux-ran"
tmux_ready="${temporary_root}/tmux-attached"
type_command "TMUX_TMPDIR='${temporary_root}/tmux' tmux -L phase4-programs new-session -d -s gate"
sleep 0.3
type_command "TMUX_TMPDIR='${temporary_root}/tmux' tmux -L phase4-programs set-hook -t gate client-attached \"run-shell 'touch ${tmux_ready}'\""
type_command "TMUX_TMPDIR='${temporary_root}/tmux' tmux -L phase4-programs attach -t gate"
wait_for_file "${tmux_ready}" "tmux client readiness"
type_command "printf tmux-ok > '${tmux_marker}'"
wait_for_file "${tmux_marker}" "tmux command"
xdotool key --clearmodifiers ctrl+b d
sleep 0.5
type_command "TMUX_TMPDIR='${temporary_root}/tmux' tmux -L phase4-programs kill-server"

kill -HUP "${child_pid}"
for _ in $(seq 1 200); do
  ! kill -0 "${app_pid}" 2>/dev/null && break
  sleep 0.1
done
if kill -0 "${app_pid}" 2>/dev/null; then
  echo "Kitmux stayed alive after the program-matrix shell exited." >&2
  cat "${log}" >&2
  exit 1
fi
wait "${app_pid}"
app_pid=""
if kill -0 "${child_pid}" 2>/dev/null; then
  echo "Kitmux left the program-matrix shell alive." >&2
  exit 1
fi
child_pid=""

echo "Phase 4 bash/zsh/fish/vim/less/tmux input gate: OK"
