#!/bin/sh
set -eu

if [ "${1:-}" = "-G" ]; then
    if [ -n "${KITMUX_SSH_WRAPPER_USED:-}" ]; then
        printf '%s\n' resolution >>"$KITMUX_SSH_WRAPPER_USED"
    fi
    printf '%s\n' \
        'host demo' \
        'hostname 127.0.0.1' \
        'user kitmux' \
        'port 22' \
        'stricthostkeychecking yes' \
        'gatewayports yes' \
        'proxyjump none' \
        'proxycommand none' \
        'localforward *:18080 127.0.0.1:22'
    exit 0
fi

if [ -n "${KITMUX_SSH_WRAPPER_USED:-}" ]; then
    printf '%s\n' connect >>"$KITMUX_SSH_WRAPPER_USED"
fi
if [ -n "${KITMUX_SSH_ARGV_LOG:-}" ]; then
    : >"$KITMUX_SSH_ARGV_LOG"
    for argument in "$@"; do
        printf '%s\n' "$argument" >>"$KITMUX_SSH_ARGV_LOG"
    done
fi
