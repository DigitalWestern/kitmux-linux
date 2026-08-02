#!/usr/bin/env python3
"""Report macOS reference drift and perform a guarded re-lock."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


LINUX_ROOT = Path(__file__).resolve().parents[2]
LOCK_PATH = LINUX_ROOT / "source-lock.json"
RELEVANT_PATHS = (
    "libkitty/",
    "patches/",
    "macos/KitmuxApp/Sources/KitmuxCore/StateSnapshot.swift",
    "macos/KitmuxApp/Sources/KitmuxCore/SplitTree.swift",
    "macos/KitmuxApp/Sources/KitmuxCore/ControlProtocol.swift",
    "macos/KitmuxApp/Tests/KitmuxCoreTests/",
    "macos/KitmuxApp/Sources/Kitmux/TerminalView.swift",
    "macos/KitmuxApp/Sources/Kitmux/PaneRuntime.swift",
    "macos/KitmuxApp/Sources/Kitmux/LibKitty.swift",
    "macos/KitmuxApp/Sources/Kitmux/ControlDispatcher.swift",
    "macos/KitmuxApp/Sources/Kitmux/SmokeSuite.swift",
)


def run(
    *args: str, cwd: Path, check: bool = True, text: bool = True
) -> subprocess.CompletedProcess[str] | subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        args, cwd=cwd, check=check, text=text, capture_output=True
    )


def git(repo: Path, *args: str, check: bool = True) -> str:
    result = run("git", *args, cwd=repo, check=check)
    return result.stdout.rstrip()


def classification(path: str) -> str:
    if (
        path == "libkitty/include/libkitty.h"
        or path.startswith("patches/")
        or path.startswith("macos/KitmuxApp/Sources/KitmuxCore/")
        or path.startswith("macos/KitmuxApp/Tests/KitmuxCoreTests/")
    ):
        return "contract-affecting"
    if (
        path.startswith("libkitty/src/")
        or path == "libkitty/py/glue.py"
        or path.startswith("libkitty/tests/")
        or path.startswith("macos/KitmuxApp/Sources/Kitmux/")
    ):
        return "behavior-affecting"
    return "irrelevant to Linux"


def mandatory_rebaseline(path: str) -> bool:
    return path == "libkitty/include/libkitty.h" or path.startswith("patches/")


def load_lock() -> dict:
    return json.loads(LOCK_PATH.read_text())


def macos_repo(lock: dict) -> Path:
    configured = os.environ.get("KITMUX_MACOS_REPO")
    return Path(configured).resolve() if configured else (
        LINUX_ROOT / lock["macos_reference"]["relative_path"]
    ).resolve()


def verify_reference(repo: Path, lock: dict) -> tuple[str, str, str]:
    if not (repo / ".git").exists():
        raise SystemExit(f"macOS reference repository not found: {repo}")
    tag = lock["macos_reference"]["tag"]
    expected = lock["macos_reference"]["commit"]
    actual = git(repo, "rev-parse", f"{tag}^{{commit}}")
    if actual != expected:
        raise SystemExit(
            f"reference tag mismatch: {tag} resolves to {actual}, lock expects {expected}"
        )
    return tag, expected, git(repo, "rev-parse", "HEAD")


def committed_changes(repo: Path, tag: str, head: str) -> list[tuple[str, str, str, str]]:
    statuses = {}
    status_output = git(
        repo,
        "diff",
        "--name-status",
        "--no-renames",
        f"{tag}..{head}",
        "--",
        *RELEVANT_PATHS,
    )
    for line in status_output.splitlines():
        status, path = line.split("\t", 1)
        statuses[path] = {"A": "added", "D": "deleted"}.get(status, "modified")

    changes = []
    numstat = git(
        repo,
        "diff",
        "--numstat",
        "--no-renames",
        f"{tag}..{head}",
        "--",
        *RELEVANT_PATHS,
    )
    for line in numstat.splitlines():
        added, deleted, path = line.split("\t", 2)
        changes.append((statuses[path], added, deleted, path))
    return changes


def dirty_changes(repo: Path) -> list[tuple[str, str]]:
    output = git(
        repo,
        "status",
        "--short",
        "--no-renames",
        "--untracked-files=all",
        "--",
        *RELEVANT_PATHS,
    )
    return [(line[:2].strip() or "modified", line[3:]) for line in output.splitlines()]


def print_report(repo: Path, lock: dict, include_patch: bool) -> None:
    tag, reference, head = verify_reference(repo, lock)
    changes = committed_changes(repo, tag, head)
    dirty = dirty_changes(repo)
    commits = git(
        repo,
        "log",
        "--format=- `%h` %s",
        f"{tag}..{head}",
        "--",
        *RELEVANT_PATHS,
    )

    print("# macOS reference-drift report")
    print()
    print(f"- Frozen tag: `{tag}`")
    print(f"- Frozen commit: `{reference}`")
    print(f"- Current macOS HEAD: `{head}`")
    print(f"- Relevant committed files: {len(changes)}")
    print(f"- Relevant uncommitted files excluded from the HEAD diff: {len(dirty)}")
    print()
    print("## Committed drift")
    print()
    if changes:
        print("| Classification | Change | Lines | Path |")
        print("| --- | --- | ---: | --- |")
        for change, added, deleted, path in changes:
            print(
                f"| {classification(path)} | {change} | +{added}/-{deleted} | `{path}` |"
            )
    else:
        print("No relevant drift.")

    print()
    print("## Relevant commits")
    print()
    print(commits or "None.")
    print()
    print("## Uncommitted macOS changes")
    print()
    if dirty:
        print("These are not part of the HEAD comparison; do not rebaseline while they exist.")
        print()
        print("| Classification | Status | Path |")
        print("| --- | --- | --- |")
        for status, path in dirty:
            print(f"| {classification(path)} | `{status}` | `{path}` |")
    else:
        print("None.")

    print()
    print("## Decision")
    print()
    if any(mandatory_rebaseline(path) for _, _, _, path in changes):
        print(
            "Mandatory rebaseline: the public libkitty header or a macOS patch changed."
        )
    elif changes:
        print(
            "Review required, but rebaselining is not automatic. Contract changes need "
            "fixture/version review; behavior changes need Linux acceptance review. "
            "A macOS view-only change does not require rebaselining."
        )
    else:
        print("No relevant committed drift; record that result at the phase boundary.")
    if dirty:
        print("Rebaselining is currently blocked by relevant uncommitted macOS changes.")

    print()
    print("## Phase-boundary ritual")
    print()
    print("1. Run this report and review `--patch` when committed drift exists.")
    print("2. Record the decision in `PORT_STATUS.md`, including no-drift results.")
    print(
        "3. For mandatory or deliberately accepted drift, tag a clean, tested macOS "
        "HEAD with a new baseline tag."
    )
    print(
        "4. From a clean Linux tree run this script with `--relock NEW_TAG`; it updates "
        "only `source-lock.json`, materializes the tag, and runs the headless gate."
    )
    print(
        "5. Review and commit only `source-lock.json`. Record the new baseline and gate "
        "result separately in `PORT_STATUS.md`."
    )

    if include_patch:
        patch = git(
            repo,
            "diff",
            "--no-ext-diff",
            f"{tag}..{head}",
            "--",
            *RELEVANT_PATHS,
        )
        print()
        print("## Restricted patch")
        print()
        print("````diff")
        print(patch)
        print("````")


def relock(repo: Path, lock: dict, tag: str) -> None:
    if git(repo, "status", "--porcelain"):
        raise SystemExit("macOS worktree must be clean before rebaselining")
    if git(LINUX_ROOT, "status", "--porcelain"):
        raise SystemExit(
            "Linux worktree must be clean so the rebaseline can change only source-lock.json"
        )

    commit = git(repo, "rev-parse", f"{tag}^{{commit}}")
    head = git(repo, "rev-parse", "HEAD")
    if commit != head:
        raise SystemExit(f"{tag} points to {commit}, not current macOS HEAD {head}")

    updated = json.loads(json.dumps(lock))
    updated["macos_reference"]["tag"] = tag
    updated["macos_reference"]["commit"] = commit
    locked_paths = [
        path for path in updated["sha256"] if not path.startswith("patches/")
    ]
    locked_paths.extend(
        git(repo, "ls-tree", "-r", "--name-only", commit, "--", "patches/")
        .splitlines()
    )
    updated["sha256"] = {}
    for path in locked_paths:
        content = run(
            "git", "show", f"{commit}:{path}", cwd=repo, text=False
        ).stdout
        updated["sha256"][path] = hashlib.sha256(content).hexdigest()
    LOCK_PATH.write_text(json.dumps(updated, indent=2) + "\n")

    subprocess.run(
        [str(LINUX_ROOT / "kitmux-linux/scripts/materialize-reference.sh")],
        cwd=LINUX_ROOT,
        check=True,
    )
    subprocess.run(
        [
            "limactl",
            "shell",
            "kitmux-linux",
            "--",
            str(LINUX_ROOT / "kitmux-linux/scripts/test-headless.sh"),
        ],
        cwd=LINUX_ROOT,
        check=True,
    )

    changed = git(LINUX_ROOT, "status", "--porcelain").splitlines()
    if changed != [" M source-lock.json"]:
        raise SystemExit(
            "rebaseline changed more than source-lock.json:\n" + "\n".join(changed)
        )
    subprocess.run(["git", "diff", "--check"], cwd=LINUX_ROOT, check=True)
    print("Rebaseline gate passed; review and commit only source-lock.json.")


def self_test() -> None:
    assert classification("libkitty/include/libkitty.h") == "contract-affecting"
    assert classification("patches/example.patch") == "contract-affecting"
    assert classification("libkitty/src/session.c") == "behavior-affecting"
    assert classification("libkitty/Makefile") == "irrelevant to Linux"
    assert mandatory_rebaseline("libkitty/include/libkitty.h")
    assert mandatory_rebaseline("patches/example.patch")
    assert not mandatory_rebaseline("macos/KitmuxApp/Sources/Kitmux/TerminalView.swift")
    print("reference-drift classification self-test: OK")


def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--patch", action="store_true", help="append the restricted patch")
    mode.add_argument("--relock", metavar="TAG", help="re-lock a clean tagged macOS HEAD")
    mode.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return 0
    lock = load_lock()
    repo = macos_repo(lock)
    if args.relock:
        relock(repo, lock, args.relock)
    else:
        print_report(repo, lock, args.patch)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as error:
        detail = (error.stderr or error.stdout or str(error)).strip()
        print(detail, file=sys.stderr)
        raise SystemExit(error.returncode)
