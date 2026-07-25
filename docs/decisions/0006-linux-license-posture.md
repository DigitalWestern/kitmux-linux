# ADR 0006: The Linux host is GPL-3.0-only free software

Status: accepted

## Context

Kitty is GPL-3.0-only. `libkitty` links Kitty's `fast_data_types` and drives
Kitty's Python inside an embedded CPython interpreter. Any Linux host that
links `libkitty` — the current C GTK host, and any future Rust host — forms a
combined work with Kitty. Distributing that combined work in a `.deb`, RPM,
AppImage, or tarball triggers GPL-3.0 obligations for the whole of it, not
only for the Kitty payload.

Earlier revisions of this plan treated licensing as a Phase 8 packaging
checklist item ("GPL source compliance"). That was wrong in a way that
mattered: the only architecture that could have kept the host proprietary is
one where the engine runs in a separate process behind a defined protocol, and
that is a Phase 4 process-model decision, not a packaging detail. Deferring the
license question to Phase 8 would have silently foreclosed the option it was
deferring.

## Decision

The Linux host is GPL-3.0-only free software. The project accepts copyleft
rather than redesigning around a separate engine process.

Consequences that are now binding, not optional:

- The repository carries the full GPL-3.0 text at [`../../LICENSE`](../../LICENSE).
- Every distributed Linux artifact ships, or offers, complete corresponding
  source for the entire combined work: the host, the model, the build scripts,
  and the exact Kitty and `libkitty` revisions used.
- The in-process architecture is licence-permitted. ADR 0005's loader boundary
  stays a technical constraint about symbol shadowing; it is no longer doing
  any licensing work.
- Public source release is a prerequisite for public binary distribution, not a
  parallel track. Phase 8 cannot ship a package from a repository nobody can
  clone and build. See ADR 0008.
- Third-party contributions are inbound-GPL-3.0-only. There is no CLA and no
  relicensing path once external contributions land.

## What this decision does not settle

- **The macOS build.** The same combined-work analysis applies to any
  distributed macOS binary that links `libkitty`. This ADR governs Linux only.
  The owner should get an actual legal opinion on the macOS distribution
  posture; nothing in this repository constitutes one.
- **Trademark and naming.** Copyleft covers the code, not the name "Kitmux".
- **Whether the project wants a public repository yet.** Open source is now
  required *before distribution*, not required *today*.

## Consequences for the plan

- Phase 0 gains a licensing slice; it is not deferred to Phase 8.
- Phase 8's evidence list keeps "GPL source compliance" but it is now a
  verification that an already-satisfied obligation is packaged correctly,
  rather than a question with an open answer.
- Phase 9's "published claims" gate must state the licence plainly.
