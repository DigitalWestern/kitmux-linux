#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
linux_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
macos_root=$(CDPATH= cd -- "$linux_root/../macos/kitmux" && pwd)
mirror="$macos_root/macos/KitmuxApp/Tests/KitmuxCoreTests/Fixtures/Portable/v1"
phase3_tmp=$(mktemp -d "${TMPDIR:-/tmp}/kitmux-phase3.XXXXXX")
trap 'rm -rf -- "$phase3_tmp"' EXIT HUP INT TERM

python3 "$linux_root/contracts/validate-fixtures.py" --mirror "$mirror"
python3 "$linux_root/contracts/validate-inventory.py" "$macos_root"
"$script_dir/test-model.sh"

cargo run --quiet --locked \
  --manifest-path "$linux_root/kitmux-linux/rust/model/Cargo.toml" \
  --example export_portable_fixtures -- \
  "$linux_root/contracts/fixtures/v1" "$phase3_tmp/linux-produced.json"

KITMUX_LINUX_FIXTURE_BUNDLE="$phase3_tmp/linux-produced.json" \
LIBRARY_PATH="$macos_root/libkitty${LIBRARY_PATH:+:$LIBRARY_PATH}" \
swift test --package-path "$macos_root/macos/KitmuxApp" \
  --scratch-path "$phase3_tmp/swift" \
  --filter PortableContractFixtureTests
