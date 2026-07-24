#!/usr/bin/env python3
"""Rebuild tracked third-party notices from the exact source archives."""

from __future__ import annotations

import argparse
import fnmatch
import hashlib
import io
import json
from pathlib import Path
import tarfile
import urllib.request


ARCHIVE_LICENSE_PATHS = {
    "cpython": ["*/LICENSE"],
    "zlib": ["*/LICENSE"],
    "bzip2": ["*/LICENSE"],
    "openssl": ["*/LICENSE.txt"],
    "expat": ["*/COPYING"],
    "xkbcommon": ["*/LICENSE"],
    "sqlite": ["*/sqlite3.c"],
    "libffi": ["*/LICENSE"],
    "ncurses": ["*/COPYING"],
    "readline": ["*/COPYING"],
    "xz": ["*/COPYING", "*/COPYING.0BSD"],
    "xxhash": ["*/LICENSE"],
    "libpng": ["*/LICENSE"],
    "lcms2": ["*/LICENSE"],
    "brotli": ["*/LICENSE"],
    "pixman": ["*/COPYING"],
    "freetype": ["*/docs/FTL.TXT", "*/docs/GPLv2.TXT"],
    "fontconfig": ["*/COPYING"],
    "cairo": ["*/COPYING-LGPL-2.1", "*/COPYING-MPL-1.1"],
    "harfbuzz": ["*/COPYING"],
    "wayland": ["*/COPYING"],
}


def archive_bytes(url: str, expected_sha256: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "kitmux-license-audit/1"})
    with urllib.request.urlopen(request) as response:
        payload = response.read()
    actual = hashlib.sha256(payload).hexdigest()
    if actual != expected_sha256:
        raise SystemExit(f"source hash mismatch for {url}: {actual} != {expected_sha256}")
    return payload


def extract_members(payload: bytes, patterns: list[str]) -> list[tuple[str, bytes]]:
    results = []
    with tarfile.open(fileobj=io.BytesIO(payload), mode="r:*") as archive:
        members = [member for member in archive.getmembers() if member.isfile()]
        for pattern in patterns:
            matches = [member for member in members if fnmatch.fnmatchcase(member.name, pattern)]
            if len(matches) > 1:
                shallowest = min(member.name.count("/") for member in matches)
                matches = [
                    member for member in matches if member.name.count("/") == shallowest
                ]
            if len(matches) != 1:
                names = [member.name for member in matches]
                raise SystemExit(f"license pattern {pattern!r} matched {names}")
            extracted = archive.extractfile(matches[0])
            if extracted is None:
                raise SystemExit(f"could not extract {matches[0].name}")
            results.append((matches[0].name, extracted.read()))
    return results


def write_notice(path: Path, source: str, members: list[tuple[str, bytes]]) -> None:
    chunks = [f"Upstream source: {source}\n".encode()]
    for name, payload in members:
        chunks.append(f"\n===== {name} =====\n\n".encode())
        chunks.append(payload.rstrip() + b"\n")
    path.write_bytes(b"".join(chunks))


def sqlite_notice(payload: bytes, patterns: list[str]) -> list[tuple[str, bytes]]:
    name, sqlite = extract_members(payload, patterns)[0]
    marker = b"** The author disclaims copyright to this source code."
    marker_position = sqlite.find(marker)
    if marker_position < 0:
        raise SystemExit("SQLite public-domain notice was not found in sqlite3.c")
    start = sqlite.rfind(b"/*", 0, marker_position)
    if start < 0:
        raise SystemExit("SQLite public-domain notice start was not found")
    end = sqlite.find(b"*/", marker_position)
    if end < 0:
        raise SystemExit("SQLite public-domain notice was not terminated")
    return [(name, sqlite[start : end + 2])]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--kitty-root", type=Path, required=True)
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text())
    components = {component["id"]: component for component in manifest["components"]}
    args.output.mkdir(parents=True, exist_ok=True)

    kitty = components["kitty"]
    write_notice(
        args.output / kitty["notice"],
        kitty["downloadLocation"],
        [("LICENSE", (args.kitty_root / "LICENSE").read_bytes())],
    )
    nerd = components["nerd-fonts-symbols"]
    nerd_archive = args.kitty_root / "dependencies" / "NerdFontsSymbolsOnly.tar.xz"
    nerd_payload = nerd_archive.read_bytes()
    if hashlib.sha256(nerd_payload).hexdigest() != nerd["sourceSha256"]:
        raise SystemExit("Nerd Fonts archive no longer matches the component manifest")
    write_notice(
        args.output / nerd["notice"],
        nerd["downloadLocation"],
        extract_members(nerd_payload, ["LICENSE"]),
    )

    for component_id, patterns in ARCHIVE_LICENSE_PATHS.items():
        component = components[component_id]
        print(f"fetching {component['name']} {component['version']}")
        payload = archive_bytes(component["downloadLocation"], component["sourceSha256"])
        members = (
            sqlite_notice(payload, patterns)
            if component_id == "sqlite"
            else extract_members(payload, patterns)
        )
        write_notice(
            args.output / component["notice"], component["downloadLocation"], members
        )

    expected = {component["notice"] for component in components.values() if component.get("notice")}
    actual = {path.name for path in args.output.iterdir() if path.is_file()}
    if expected != actual:
        raise SystemExit(f"notice set mismatch: expected {sorted(expected)}, got {sorted(actual)}")
    print(f"wrote {len(actual)} verified license notices to {args.output}")


if __name__ == "__main__":
    main()
