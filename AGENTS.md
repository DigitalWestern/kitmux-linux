# Kitmux Linux Agent Instructions

This directory is the implementation home for the experimental Linux host.
The adjacent macOS checkout is a behavioral reference, not code to translate
line by line.

## Read this first

Read these files in order before changing Linux-port code or plans:

1. [`PORT_STATUS.md`](PORT_STATUS.md) — current checkpoint, next slice, and
   blockers.
2. [`NEXT_STEPS.md`](NEXT_STEPS.md) — exact implementation sequence from the
   current checkpoint.
3. [`LINUX_PORT_PLAN.md`](LINUX_PORT_PLAN.md) — dependency-ordered work and
   exit gates.
4. [`../macos/kitmux/AGENTS.md`](../macos/kitmux/AGENTS.md) and
   [`../macos/kitmux/docs/AGENT_HANDOFF.md`](../macos/kitmux/docs/AGENT_HANDOFF.md)
   — current macOS behavior and ownership.

Then inspect the live Git status and current commits. Recorded hashes are dated
observations, not proof that a checkout is still unchanged.

## Working rules

- Work on one numbered slice from the active milestone at a time.
- Do not begin a later slice until the current slice's exit gate passes or the
  blocker is recorded in `PORT_STATUS.md`.
- Keep the macOS checkout read-only unless the assigned slice explicitly
  includes baseline cleanup or cross-platform fixture work.
- Never copy from an uncommitted macOS file into the Linux implementation.
- Reuse one authoritative `libkitty` source. Temporary read-only consumption
  during a spike is allowed; manually maintained duplicate copies are not.
- Share schemas, fixtures, command identifiers, and observable behavior before
  attempting to share the Swift product core.
- Treat GTK 4 as a candidate until the complete Slice 2.3 decision gate
  passes. Slice 2.1 alone did not select it.
- Before adding a file to the GTK spike, classify it as durable or disposable
  per ADR 0007. Durable code is display-free, has a headless test, and stays C
  behind the FFI boundary. Disposable code lives in `gtk_terminal_host.c`.
- Spike work answers "can this toolkit do X at all?". Implementing how Kitmux
  does X is product work and belongs in Phase 4. If VTE already ships the
  behavior on GTK, it is not a kill-spike question.
- The Linux host is GPL-3.0-only (ADR 0006). Do not add a dependency whose
  licence is incompatible with GPL-3.0, and do not describe the Linux port as
  closed-source or dual-licensable.
- New gates take their display, architecture, and paths from the environment.
  Do not hard-code this VM; see ADR 0008.
- Any new pinned input gets a hash in `source-lock.json` in the same commit
  that introduces it.
- Keep browser work out of the terminal-first alpha.
- Start release-layout and clean-machine checks with the engine. A build on a
  developer machine is not packaging proof.
- Preserve user changes. Ask before deletion, history rewriting, or abandoning
  the dirty macOS refactor.

## Source-of-truth order

When documents disagree, use this order:

1. Observable behavior and tests at the tagged clean macOS baseline.
2. The public `libkitty` C header and frozen cross-platform fixtures.
3. The macOS handoff and active user-facing documentation.
4. The active Linux plan.

AppKit lifecycle details, macOS paths, and macOS shortcuts are not portable
requirements unless a contract or feature-inventory entry says so.

## Finishing a slice

Before reporting completion:

1. Run the slice's fast tests and milestone-specific gate.
2. Record the exact commands and results in `PORT_STATUS.md`.
3. Update the feature inventory, support matrix, or decision record if the
   slice changes one.
4. State source-tested, GUI-tested, package-tested, and clean-machine-tested
   evidence separately.
5. Leave the next slice explicit.

Keep detailed progress in `PORT_STATUS.md`, not in this instruction file.
