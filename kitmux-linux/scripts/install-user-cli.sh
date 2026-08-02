#!/usr/bin/env bash
set -euo pipefail

source_binary="${1:-}"
if [[ -z "${source_binary}" || ! -x "${source_binary}" ]]; then
  echo "usage: install-user-cli.sh /path/to/kitmuxctl" >&2
  echo "The source must be an executable from a built or installed Kitmux runtime." >&2
  exit 2
fi

bin_home="${XDG_BIN_HOME:-${HOME}/.local/bin}"
mkdir -p -- "${bin_home}"
target="${bin_home}/kitmuxctl"
ln -sfn -- "$(realpath -- "${source_binary}")" "${target}"
echo "Installed ${target} -> $(realpath -- "${source_binary}")"
echo "Keep ${bin_home} on PATH; a missing target is reported instead of silently falling back."
