# ADR 0008: Reproducibility obligations and when they become due

Status: accepted

## Context

Every gate this project has passed was run by hand, by one person, in two
hand-provisioned Lima VMs, on one Apple-silicon macOS machine. That was the
right trade while the question was "does the engine work on Linux at all."

It stops being the right trade at the point where the project needs a second
pair of hands or a second architecture — and ADR 0006 makes that point
concrete, because public binary distribution now requires public source, and
public source that nobody can build is not a contribution surface.

This ADR does not create CI today. The repository has no remote. It records
what is currently unreproducible, and fixes the moment each item comes due, so
that no gate quietly rots in the meantime.

## Known reproducibility defects

These were real, verified properties of the tree as of 2026-07-25. Each is
tracked in `PORT_STATUS.md`; R3 closed on 2026-07-28.

### R1 — The repository cannot be built standalone

`scripts/materialize-reference.sh` requires the private macOS repository at
`../macos/kitmux`, at tag
`macos-linux-port-baseline-2026-08-02-v0.21`, and extracts `libkitty/` and
`patches/` from it. `kitmux-linux/patches/` is an empty directory in this tree.
A clone of this repository alone fails at the first step, and the engine glue
and Kitty patches — the load-bearing inputs — are not present in it.

Due: at the monorepo migration, which ADR 0006 promotes from "recommended
eventually" to a prerequisite for Phase 8. See
[`../MONOREPO_MIGRATION.md`](../MONOREPO_MIGRATION.md).

### R2 — No automated gate

Nothing runs on commit. `ci/Containerfile.ubuntu` and `ci/Containerfile.fedora`
already contain the hard part; `scripts/test-clean-containers.sh` already
drives them. Only the trigger is missing.

Due: with the first Git remote. The headless container gate is the one to
automate first because it is already hermetic and needs no display.

### R3 — The desktop gate is bound to one VM, not to a display — closed

`scripts/test-desktop.sh` requires `KITMUX_VNC_DISPLAY`, a noVNC endpoint on
port 6080, XFCE, and this project's specific VNC session. It also mutates
session-global state while it runs — X auto-repeat, the active XKB layout, and
the active IBus engine — and restores only the first two on exit.

That is acceptable for a dedicated project VM and unacceptable for anything
else: no contributor can run it against their own desktop, and it cannot run
under Xvfb or headless Weston in a container.

Due: before Phase 4. Separate "needs a display" from "needs *this* display",
so the gate takes a `DISPLAY`/`WAYLAND_DISPLAY` it did not create. Do not
attempt this edit without being able to run the gate afterwards.

Resolution, 2026-07-28: `test-desktop.sh` now accepts a caller-supplied
`DISPLAY`, skips noVNC when no port is requested, and restores the original X
repeat state/rate, complete XKB configuration, and active IBus engine. The
full X11 and nested-Wayland gate passed after the change. Starting the local
XFCE/VNC session remains a convenience, not a gate requirement.

### R4 — x86_64 inputs are not locked

Both Tier-1 environments in `support-matrix.yml` are x86_64. `CMakeLists.txt`
resolves `linux-64` for that architecture, but `source-lock.json` records a
SHA-256 for `linux-arm64.tar.xz` only. x86_64 is therefore not merely untested:
its dependency bundle is unpinned, so an x86_64 build would not be reproducible
even if it succeeded.

Due: with R2, since the first automated gate should cover both architectures
or explicitly declare that it does not.

Resolved 2026-07-28, ahead of that date, because it was a one-value edit and
holding it hostage to CI kept a Tier-1 architecture unpinned for no reason.
`linux-64.tar.xz` is locked from the same upstream build as the arm64 bundle;
the evidence and the exact provenance are in `PORT_STATUS.md`. The rule below
stands: this pins the input, it does not claim the architecture. Locking also
exposed a defect this ADR did not name — the bundle URL carries no version or
content address, so both hashes go stale together whenever upstream rebuilds.
That is tracked as R5 in `PORT_STATUS.md` and is due with R1.

## Rules that hold now

- A gate that cannot be run by someone other than the author is evidence with
  an expiry date. Record it as such in `PORT_STATUS.md`.
- New gates take their display, architecture, and paths from the environment.
  Do not add another script that hard-codes this VM.
- Any new pinned input gets a hash in `source-lock.json` in the same commit
  that introduces it.
- `release-tools.py verify-inputs` must enforce every recorded dependency
  bundle and Cargo lockfile hash before a release-shaped build starts. The
  clean-target gate separately enforces its digest-pinned container base.
- Locking x86_64 inputs is not permission to claim x86_64 support. Support
  requires a passing gate.

## Consequences

- Phase 8 gains an explicit dependency on R1 and R2. A native package built
  from an unreproducible tree cannot satisfy ADR 0006's source obligation.
- Phase 9's maintainer-naming gate is unsatisfiable while the bus factor is
  one; R1 through R3 are what reduce it.
