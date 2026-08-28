# Kitmux Linux next steps

**Next gate: Phase 6 beta hardware evidence and final release blockers.**

Phases 0 through 5 and Slices 6.1–6.3 are closed. The clean tag
`macos-linux-port-baseline-2026-08-02-v0.21` remains locked. Completed evidence
lives in [`PORT_STATUS.md`](PORT_STATUS.md); phase scope and exit gates live in
[`LINUX_PORT_PLAN.md`](LINUX_PORT_PLAN.md); commands live in
[`docs/LINUX_DEVELOPMENT.md`](docs/LINUX_DEVELOPMENT.md).

## Preconditions

1. Read [`AGENTS.md`](AGENTS.md), the header and blockers sections of
   `PORT_STATUS.md`, Phase 6 and Phase 8 in `LINUX_PORT_PLAN.md`, and ADRs
   0006, 0007, and 0008.
2. Inspect `git status --short` and commits newer than the latest evidence
   entry. Preserve unrelated worktree changes.
3. Run `kitmux-linux/scripts/report-reference-drift.py` and record the result
   before the next phase boundary. Do not relock while the adjacent macOS
   checkout has relevant uncommitted changes.

## Completed gates

- Slice 6.3 resume/recovery and the terminal-only Phase 7 decision are closed.
- ADR 0008 R1 is closed for ARM64, and R2 closed on 2026-08-27: the
  standalone workflow passed on GitHub-hosted runners (both jobs); the run
  reference is in `PORT_STATUS.md`.
- The ARM64 and font dependency bundles are mirrored and checksum-verified;
  x86_64 has an explicit unlocked fallback build while its historical bundle
  is still unavailable.
- Reproducible ARM64 tarball and `.deb` artifacts pass the fresh-VM install,
  launch, upgrade, downgrade, reinstall, and uninstall gate.
- Accessibility/product coverage and the written threat-model review are
  complete for the terminal-alpha scope.
- The complete local aggregate, ARM64 package lifecycle, clean-target launch,
  and twice-repeated Ubuntu/Fedora clean-container release gates passed on
  2026-08-23.

## Remaining order

1. Prove one physical Mesa GPU — work stream A of
   [`docs/BETA_EVIDENCE_PLAN.md`](docs/BETA_EVIDENCE_PLAN.md). Blocked on the
   Task A0 host decision; a llvmpipe result is correctness evidence only.
2. Lock and gate x86_64 — work stream B of the same plan. Blocked on the
   Task B0 re-lock approval; after it, the locked gate runs on the existing
   CI runner with no new hardware.
3. Close the open menu-bar traversal check — the deterministic
   `test-phase5-product.sh` F10/arrow/Escape failure. The completion plan is
   [`docs/MENUBAR_PLAN.md`](docs/MENUBAR_PLAN.md) section 8; not blocked on
   anything.
4. Decide and document the remaining release bar: real SSH/network
   authentication, power-loss recovery, desktop-menu interaction, package
   signing, vulnerability review, full AT-SPI coverage, and release-maintainer
   ownership. The local long-soak, clean-target, and package lifecycle gates
   already passed.

## Phase 7 decision

The beta scope is terminal-only. Do not build browser panes, portal
integrations, or their additional X11/Wayland safety tests unless the product
scope is explicitly changed. The existing WebKitGTK coexistence probe remains
a spike result, not browser product functionality.

## Do not start

- Live macOS state import or restore; Slice 3.3's preview is closed.
- Repository migration; it needs its own session.
- A second SSH feature slice; Slices 6.2 and 6.3 are closed.
- Any packaging promotion before R2, x86_64, physical-GPU, signing, and security
  evidence have current results.
