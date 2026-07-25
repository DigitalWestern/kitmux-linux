#!/usr/bin/env python3
"""Validate the portable contract corpus and its optional macOS mirror."""

from __future__ import annotations

import argparse
import json
import pathlib
import sys
from typing import Any


ALLOWED_DISPOSITIONS = {"accept", "repair", "reject", "ignore", "preserve"}
FORBIDDEN_PORTABLE_KEYS = {
    "absolutePath",
    "agentSocket",
    "browserCookies",
    "browserData",
    "fontName",
    "keychainReference",
    "packagePath",
    "socketPath",
}
FORBIDDEN_PATH_MARKERS = ("/Users/", "/home/", "/private/", "C:\\Users\\")


def bounded_integer_map(value: Any) -> bool:
    return (
        isinstance(value, dict)
        and bool(value)
        and all(
            isinstance(key, str)
            and isinstance(item, int)
            and not isinstance(item, bool)
            and item >= 0
            for key, item in value.items()
        )
        and any(item > 0 for item in value.values())
    )


def walk(value: Any):
    yield value
    if isinstance(value, dict):
        for key, item in value.items():
            yield key
            yield from walk(item)
    elif isinstance(value, list):
        for item in value:
            yield from walk(item)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--mirror",
        type=pathlib.Path,
        help="macOS Portable/v1 resource directory that must be byte-identical",
    )
    args = parser.parse_args()

    root = pathlib.Path(__file__).resolve().parent / "fixtures" / "v1"
    failures: list[str] = []
    try:
        manifest = json.loads((root / "manifest.json").read_text())
    except (OSError, json.JSONDecodeError) as error:
        print(f"could not read fixture manifest: {error}", file=sys.stderr)
        return 1

    if manifest.get("fixtureVersion") != 1:
        failures.append("manifest: fixtureVersion must be 1")
    maximum = manifest.get("maximumFixtureBytes")
    if not isinstance(maximum, int) or isinstance(maximum, bool) or maximum <= 0:
        failures.append("manifest: maximumFixtureBytes must be a positive integer")
        maximum = 0

    contracts = manifest.get("contracts")
    if not isinstance(contracts, list) or not contracts:
        failures.append("manifest: contracts must be a non-empty array")
        contracts = []

    contract_ids: set[str] = set()
    files: list[pathlib.Path] = [root / "manifest.json"]
    case_ids: set[str] = set()
    case_count = 0

    for contract in contracts:
        if not isinstance(contract, dict):
            failures.append("manifest: every contract must be an object")
            continue
        contract_id = contract.get("id")
        filename = contract.get("file")
        if not isinstance(contract_id, str) or not contract_id:
            failures.append("manifest: contract id must be a non-empty string")
            continue
        if contract_id in contract_ids:
            failures.append(f"manifest: duplicate contract id {contract_id}")
        contract_ids.add(contract_id)
        if (
            not isinstance(filename, str)
            or pathlib.PurePath(filename).name != filename
            or not filename.endswith(".json")
        ):
            failures.append(f"{contract_id}: file must be one JSON basename")
            continue

        path = root / filename
        files.append(path)
        try:
            raw = path.read_bytes()
            document = json.loads(raw)
        except (OSError, json.JSONDecodeError) as error:
            failures.append(f"{contract_id}: cannot read {filename}: {error}")
            continue

        if maximum and len(raw) > maximum:
            failures.append(
                f"{contract_id}: {len(raw)} bytes exceeds fixture limit {maximum}"
            )
        if document.get("fixtureVersion") != 1:
            failures.append(f"{contract_id}: fixtureVersion must be 1")
        if not bounded_integer_map(document.get("bounds")):
            failures.append(
                f"{contract_id}: bounds must contain non-negative integers"
            )

        cases = document.get("cases")
        if not isinstance(cases, list) or not cases:
            failures.append(f"{contract_id}: cases must be a non-empty array")
            continue
        for case in cases:
            case_count += 1
            if not isinstance(case, dict):
                failures.append(f"{contract_id}: every case must be an object")
                continue
            case_id = case.get("id")
            disposition = case.get("disposition")
            if not isinstance(case_id, str) or not case_id:
                failures.append(f"{contract_id}: case id must be a non-empty string")
                continue
            qualified = f"{contract_id}.{case_id}"
            if qualified in case_ids:
                failures.append(f"duplicate case id {qualified}")
            case_ids.add(qualified)
            if disposition not in ALLOWED_DISPOSITIONS:
                failures.append(f"{qualified}: invalid disposition {disposition!r}")

            for value in walk(case):
                if isinstance(value, str):
                    if value in FORBIDDEN_PORTABLE_KEYS:
                        failures.append(f"{qualified}: forbidden portable key {value}")
                    if any(marker in value for marker in FORBIDDEN_PATH_MARKERS):
                        failures.append(f"{qualified}: contains a platform user path")

            if (
                disposition == "accept"
                and contract_id == "ssh-profile-review"
                and isinstance(case.get("input"), dict)
            ):
                encoded = json.dumps(case["input"])
                if '"remoteCommand"' in encoded:
                    failures.append(
                        f"{qualified}: accepted portable profile stores a command"
                    )

    declared = {path.name for path in files}
    actual = {path.name for path in root.glob("*.json")}
    extras = sorted(actual - declared)
    missing = sorted(declared - actual)
    if extras:
        failures.append(f"manifest: undeclared JSON files: {', '.join(extras)}")
    if missing:
        failures.append(f"manifest: missing JSON files: {', '.join(missing)}")

    if args.mirror:
        mirror = args.mirror.resolve()
        for source in files:
            target = mirror / source.name
            if not target.is_file():
                failures.append(f"mirror: missing {target}")
            elif source.read_bytes() != target.read_bytes():
                failures.append(f"mirror: {source.name} differs byte-for-byte")
        mirror_json = {path.name for path in mirror.glob("*.json")}
        if mirror_json != declared:
            failures.append("mirror: JSON file set differs from the manifest")

    print(
        f"{len(contract_ids)} contracts, {case_count} cases, "
        f"{len(files)} versioned JSON files"
    )
    if args.mirror:
        print(f"checked byte-identical macOS mirror at {args.mirror.resolve()}")
    if failures:
        print(f"\n{len(failures)} failure(s):")
        for failure in failures:
            print(f"  {failure}")
        return 1
    print("portable fixtures OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
