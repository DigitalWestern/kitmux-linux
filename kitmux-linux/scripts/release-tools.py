#!/usr/bin/env python3
"""Build-time dependency closure, SPDX generation, and release auditing."""

from __future__ import annotations

import argparse
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


SYSTEM_SONAMES = {
    "libX11-xcb.so.1",
    "libX11.so.6",
    "libXcursor.so.1",
    "libc.so.6",
    "libdbus-1.so.3",
    "libdl.so.2",
    "libgcc_s.so.1",
    "libm.so.6",
    "libpthread.so.0",
    "libuuid.so.1",
    "libxcb-xkb.so.1",
    "libxcb.so.1",
}


def fail(message: str) -> None:
    raise SystemExit(message)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha1(path: Path) -> str:
    digest = hashlib.sha1(usedforsecurity=False)
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def is_elf(path: Path) -> bool:
    if not path.is_file() or path.is_symlink():
        return False
    with path.open("rb") as stream:
        return stream.read(4) == b"\x7fELF"


def needed(path: Path) -> list[str]:
    output = subprocess.check_output(
        ["patchelf", "--print-needed", str(path)], text=True
    )
    return [line for line in output.splitlines() if line]


def copy_link_chain(source: Path, destination_dir: Path) -> Path:
    current = source
    seen: set[Path] = set()
    while current.is_symlink():
        if current in seen:
            fail(f"dependency symlink cycle: {source}")
        seen.add(current)
        target = os.readlink(current)
        destination = destination_dir / current.name
        if destination.exists() or destination.is_symlink():
            destination.unlink()
        destination.symlink_to(target)
        current = current.parent / target
    if not current.is_file():
        fail(f"dependency symlink target is missing: {source} -> {current}")
    destination = destination_dir / current.name
    shutil.copy2(current, destination)
    return destination


def copy_dependencies(args: argparse.Namespace) -> None:
    dependency_lib = args.dependency_lib.resolve()
    runtime_lib = args.runtime_lib.resolve()
    runtime_lib.mkdir(parents=True, exist_ok=True)
    queue = [path.resolve() for path in args.root]
    inspected: set[Path] = set()
    bundled: set[str] = set()
    system: set[str] = set()

    while queue:
        elf = queue.pop()
        if elf in inspected:
            continue
        if not is_elf(elf):
            fail(f"dependency root is not an ELF file: {elf}")
        inspected.add(elf)
        for soname in needed(elf):
            existing = runtime_lib / soname
            if existing.exists():
                queue.append(existing.resolve())
                continue
            source = dependency_lib / soname
            if source.exists():
                copied = copy_link_chain(source, runtime_lib)
                bundled.add(soname)
                queue.append(copied.resolve())
                continue
            if soname.startswith("ld-linux-") or soname in SYSTEM_SONAMES:
                system.add(soname)
                continue
            fail(f"undeclared dependency {soname} required by {elf}")

    report = {
        "schemaVersion": 1,
        "bundledSonames": sorted(bundled),
        "systemSonames": sorted(system),
    }
    if args.report:
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(
        f"dependency closure: {len(bundled)} bundled SONAMEs, "
        f"{len(system)} system SONAMEs"
    )


def load_manifest(path: Path) -> dict:
    manifest = json.loads(path.read_text())
    if manifest.get("schemaVersion") != 1:
        fail(f"unsupported component manifest schema: {path}")
    ids = [component["id"] for component in manifest["components"]]
    if len(ids) != len(set(ids)):
        fail("component manifest contains duplicate ids")
    return manifest


def regular_runtime_files(runtime: Path) -> list[Path]:
    excluded = {"share/SHA256SUMS", "share/kitmux-engine.spdx.json"}
    return sorted(
        path
        for path in runtime.rglob("*")
        if path.is_file()
        and not path.is_symlink()
        and path.relative_to(runtime).as_posix() not in excluded
    )


def file_owners(runtime: Path, manifest: dict) -> dict[Path, dict]:
    owners: dict[Path, dict] = {}
    payload_counts = {component["id"]: 0 for component in manifest["components"]}
    for path in regular_runtime_files(runtime):
        relative = path.relative_to(runtime).as_posix()
        matches = [
            component
            for component in manifest["components"]
            if any(fnmatch.fnmatchcase(relative, pattern) for pattern in component["files"])
        ]
        if len(matches) != 1:
            fail(
                f"runtime file must map to exactly one component: {relative} "
                f"(matched {[item['id'] for item in matches]})"
            )
        owners[path] = matches[0]
        if not relative.startswith("share/licenses/"):
            payload_counts[matches[0]["id"]] += 1
    empty = [name for name, count in payload_counts.items() if count == 0]
    if empty:
        fail(f"manifest components without runtime payloads: {', '.join(empty)}")
    return owners


def spdx_id(prefix: str, value: str) -> str:
    clean = "".join(character if character.isalnum() or character in ".-" else "-" for character in value)
    return f"SPDXRef-{prefix}-{clean}"


def generate_sbom(args: argparse.Namespace) -> None:
    runtime = args.runtime.resolve()
    manifest = load_manifest(args.manifest)
    owners = file_owners(runtime, manifest)
    file_hashes = {
        path: sha256(path)
        for path in owners
    }
    namespace_seed = hashlib.sha256(args.manifest.read_bytes())
    for path in sorted(file_hashes):
        namespace_seed.update(path.relative_to(runtime).as_posix().encode())
        namespace_seed.update(file_hashes[path].encode())
    namespace = namespace_seed.hexdigest()

    packages = []
    files = []
    relationships = []
    primary_id = spdx_id("Package", "kitmux-engine")
    for component in manifest["components"]:
        package_id = spdx_id("Package", component["id"])
        component_files = [path for path, owner in owners.items() if owner is component]
        verification_input = "".join(sorted(sha1(path) for path in component_files))
        package = {
            "SPDXID": package_id,
            "name": component["name"],
            "versionInfo": component["version"],
            "downloadLocation": component["downloadLocation"],
            "filesAnalyzed": True,
            "licenseConcluded": component["license"],
            "licenseDeclared": component["license"],
            "licenseInfoFromFiles": [component["license"]],
            "copyrightText": "NOASSERTION",
            "packageVerificationCode": {
                "packageVerificationCodeValue": hashlib.sha1(
                    verification_input.encode(), usedforsecurity=False
                ).hexdigest()
            },
        }
        if component.get("sourceSha256"):
            package["checksums"] = [
                {"algorithm": "SHA256", "checksumValue": component["sourceSha256"]}
            ]
        packages.append(package)
        if package_id == primary_id:
            relationships.append(
                {
                    "spdxElementId": "SPDXRef-DOCUMENT",
                    "relationshipType": "DESCRIBES",
                    "relatedSpdxElement": package_id,
                }
            )
        else:
            relationships.append(
                {
                    "spdxElementId": primary_id,
                    "relationshipType": "DEPENDS_ON",
                    "relatedSpdxElement": package_id,
                }
            )

    for path, component in sorted(owners.items()):
        relative = path.relative_to(runtime).as_posix()
        file_id = spdx_id("File", relative)
        files.append(
            {
                "SPDXID": file_id,
                "fileName": f"./{relative}",
                "checksums": [
                    {"algorithm": "SHA256", "checksumValue": file_hashes[path]},
                    {"algorithm": "SHA1", "checksumValue": sha1(path)},
                ],
                "licenseConcluded": "NOASSERTION",
                "licenseInfoInFiles": ["NOASSERTION"],
                "copyrightText": "NOASSERTION",
            }
        )
        relationships.append(
            {
                "spdxElementId": spdx_id("Package", component["id"]),
                "relationshipType": "CONTAINS",
                "relatedSpdxElement": file_id,
            }
        )

    document = {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": manifest["document"]["name"],
        "documentNamespace": f"https://kitmux.local/spdx/{namespace}",
        "creationInfo": {
            "created": manifest["document"]["created"],
            "creators": [manifest["document"]["supplier"]],
        },
        "hasExtractedLicensingInfos": [
            {
                "licenseId": "LicenseRef-SQLite-Public-Domain",
                "name": "SQLite public-domain dedication",
                "extractedText": "See share/licenses/sqlite.txt in this runtime.",
            }
        ],
        "packages": packages,
        "files": files,
        "relationships": relationships,
    }
    args.output.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")
    print(f"SPDX SBOM: {len(packages)} packages, {len(files)} files")


def verify(args: argparse.Namespace) -> None:
    runtime = args.runtime.resolve()
    manifest = load_manifest(args.manifest)
    owners = file_owners(runtime, manifest)
    notices = runtime / "share" / "licenses"
    for component in manifest["components"]:
        notice = component.get("notice")
        if notice and not (notices / notice).is_file():
            fail(f"missing license notice for {component['id']}: {notice}")

    sbom_path = runtime / "share" / "kitmux-engine.spdx.json"
    sbom = json.loads(sbom_path.read_text())
    if sbom.get("spdxVersion") != "SPDX-2.3":
        fail("runtime SBOM is not SPDX 2.3 JSON")
    expected_packages = {spdx_id("Package", item["id"]) for item in manifest["components"]}
    actual_packages = {item["SPDXID"] for item in sbom.get("packages", [])}
    if expected_packages != actual_packages:
        fail("runtime SBOM package set does not match component manifest")
    expected_files = {f"./{path.relative_to(runtime).as_posix()}" for path in owners}
    actual_files = {item["fileName"] for item in sbom.get("files", [])}
    if expected_files != actual_files:
        fail("runtime SBOM file set does not match release payload")
    sbom_files = {item["fileName"]: item for item in sbom["files"]}
    for path in owners:
        relative = f"./{path.relative_to(runtime).as_posix()}"
        recorded = sbom_files[relative]["checksums"][0]["checksumValue"]
        if recorded != sha256(path):
            fail(f"runtime SBOM checksum mismatch: {relative}")
    print(
        f"release metadata: {len(expected_packages)} components and "
        f"{len(expected_files)} files verified"
    )


def verify_inputs(args: argparse.Namespace) -> None:
    linux_root = args.linux_root.resolve()
    lock = json.loads((linux_root / "source-lock.json").read_text())
    dependency_root = linux_root / ".source" / "kitty" / "dependencies"
    expected_bundles = lock.get("kitty_dependency_bundles", {})
    required_bundles = [f"{args.platform}.tar.xz", "NerdFontsSymbolsOnly.tar.xz"]
    for name in required_bundles:
        expected = expected_bundles.get(name)
        if not expected:
            fail(f"source-lock.json has no checksum for required bundle {name}")
        path = dependency_root / name
        if not path.is_file():
            fail(f"locked dependency bundle is missing: {path}")
        actual = sha256(path)
        if actual != expected:
            fail(f"dependency bundle hash mismatch for {name}: {actual} != {expected}")

    manifest = load_manifest(args.manifest)
    kitty_sources = (linux_root / ".source" / "kitty" / "bypy" / "sources.json").read_text()
    checked_sources = 0
    for component in manifest["components"]:
        source_hash = component.get("sourceSha256")
        if not source_hash or component["id"] == "nerd-fonts-symbols":
            continue
        if f"sha256:{source_hash}" not in kitty_sources:
            fail(
                f"component source hash is not present in Kitty's pinned sources: "
                f"{component['id']}"
            )
        checked_sources += 1
    print(
        f"locked release inputs: {len(required_bundles)} bundles and "
        f"{checked_sources} source archives verified"
    )


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser()
    subparsers = result.add_subparsers(dest="command", required=True)

    copy = subparsers.add_parser("copy-dependencies")
    copy.add_argument("--dependency-lib", type=Path, required=True)
    copy.add_argument("--runtime-lib", type=Path, required=True)
    copy.add_argument("--report", type=Path)
    copy.add_argument("--root", type=Path, action="append", required=True)
    copy.set_defaults(function=copy_dependencies)

    generate = subparsers.add_parser("generate-sbom")
    generate.add_argument("--runtime", type=Path, required=True)
    generate.add_argument("--manifest", type=Path, required=True)
    generate.add_argument("--output", type=Path, required=True)
    generate.set_defaults(function=generate_sbom)

    audit = subparsers.add_parser("verify")
    audit.add_argument("--runtime", type=Path, required=True)
    audit.add_argument("--manifest", type=Path, required=True)
    audit.set_defaults(function=verify)

    inputs = subparsers.add_parser("verify-inputs")
    inputs.add_argument("--linux-root", type=Path, required=True)
    inputs.add_argument("--platform", required=True)
    inputs.add_argument("--manifest", type=Path, required=True)
    inputs.set_defaults(function=verify_inputs)
    return result


def main() -> None:
    args = parser().parse_args()
    args.function(args)


if __name__ == "__main__":
    main()
