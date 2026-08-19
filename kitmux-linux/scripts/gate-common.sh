#!/usr/bin/env bash

# Shared preflight for every runnable test gate. Source this file after the
# caller has enabled its own strict shell options.
gate_common_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
gate_common_linux_root="$(cd -- "${gate_common_dir}/../.." && pwd)"
if [[ "${KITMUX_INVENTORY_VALIDATED:-0}" == "1" ]]; then
  return 0
fi
export KITMUX_INVENTORY_VALIDATED=1
if [[ -n "${KITMUX_MACOS_ROOT:-}" ]]; then
  python3 "${gate_common_linux_root}/contracts/validate-inventory.py" \
    "${KITMUX_MACOS_ROOT}"
else
  python3 "${gate_common_linux_root}/contracts/validate-inventory.py"
fi
