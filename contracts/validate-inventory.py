#!/usr/bin/env python3
"""Check that feature-inventory.json stays honest.

Verifies structure, unique IDs, resolvable dependencies, and — most
importantly — that every `macos_sources` and `macos_tests` reference points at
a file that exists in the tagged macOS baseline, and that every
`path::testName` reference names a test function that is actually there.

A traceability claim nobody checks decays into decoration. Run this whenever
the inventory or the macOS baseline changes.

    contracts/validate-inventory.py [path-to-macos-checkout]

Exits non-zero on any failure.
"""
import json
import pathlib
import re
import sys

VALID_CLASSIFICATIONS = {
    "terminal-alpha", "beta", "later", "intentionally-omitted",
}
REQUIRED_FIELDS = (
    "id", "area", "classification", "behavior", "macos_sources",
    "macos_tests", "linux_acceptance", "dependencies", "translation",
    "linux_status",
)


def main() -> int:
    here = pathlib.Path(__file__).resolve().parent
    macos = pathlib.Path(
        sys.argv[1] if len(sys.argv) > 1 else here.parent / ".." / "macos" / "kitmux"
    ).resolve()

    document = json.loads((here / "feature-inventory.json").read_text())
    features = document["features"]
    areas = {area["id"] for area in document["areas"]}
    ids = {feature["id"] for feature in features}
    failures: list[str] = []

    if len(ids) != len(features):
        failures.append("duplicate feature IDs")

    for feature in features:
        fid = feature.get("id", "<missing id>")
        for field in REQUIRED_FIELDS:
            if field not in feature:
                failures.append(f"{fid}: missing field {field}")
        if feature.get("area") not in areas:
            failures.append(f"{fid}: unknown area {feature.get('area')!r}")
        if feature.get("classification") not in VALID_CLASSIFICATIONS:
            failures.append(
                f"{fid}: unknown classification {feature.get('classification')!r}")
        if not feature.get("linux_acceptance", "").strip():
            failures.append(f"{fid}: empty linux_acceptance")
        for dependency in feature.get("dependencies", []):
            # Dependencies may name another feature or an external capability.
            if "." in dependency and " " not in dependency and dependency not in ids:
                if not dependency.startswith("contracts."):
                    failures.append(f"{fid}: dangling dependency {dependency!r}")

    if not macos.is_dir():
        print(f"macOS baseline not found at {macos}; skipped reference checks.")
        print("Pass its path as the first argument to check them.")
    else:
        resolved = 0
        for feature in features:
            refs = feature.get("macos_sources", []) + feature.get("macos_tests", [])
            for ref in refs:
                path, _, test_name = ref.partition("::")
                target = macos / path
                if not target.exists():
                    failures.append(f"{feature['id']}: missing {path}")
                    continue
                if test_name and not re.search(
                    r"func\s+" + re.escape(test_name) + r"\b", target.read_text()
                ):
                    failures.append(
                        f"{feature['id']}: {path} has no test {test_name}")
                    continue
                resolved += 1
        print(f"resolved {resolved} macOS source and test references")

    alpha = sum(1 for f in features if f["classification"] == "terminal-alpha")
    print(f"{len(features)} features across {len(areas)} areas ({alpha} terminal-alpha)")

    if failures:
        print(f"\n{len(failures)} failure(s):")
        for failure in failures:
            print(f"  {failure}")
        return 1
    print("inventory OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
