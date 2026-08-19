# Kitmux Linux next steps

**Next slice: 6.3 — Resume and recovery.**

Phases 0 through 5 and Slices 6.1–6.2 are closed. The clean tag
`macos-linux-port-baseline-2026-08-02-v0.21` is locked and its Ubuntu headless
gate passes.

This file says what to do next and nothing else. Completed-slice evidence lives
in [`PORT_STATUS.md`](PORT_STATUS.md); phase scope and exit gates live in
[`LINUX_PORT_PLAN.md`](LINUX_PORT_PLAN.md); commands live in
[`docs/LINUX_DEVELOPMENT.md`](docs/LINUX_DEVELOPMENT.md).

## Preconditions

1. Read [`AGENTS.md`](AGENTS.md), the header and blockers sections of
   `PORT_STATUS.md`, Phase 6 in `LINUX_PORT_PLAN.md`, and ADRs 0006, 0007, and
   0008.
2. Inspect `git status --short` and any commits newer than the latest
   `PORT_STATUS.md` evidence entry. Preserve unrelated worktree changes.
3. Re-run the locked reference materialization check before product work:

   ```sh
   kitmux-linux/scripts/materialize-reference.sh
   ```

4. If the tagged baseline no longer materializes exactly, stop and record the
   drift before changing contracts or Linux behavior.

## Slice 6.3 — Resume and recovery

Scope and exit criteria are in
[Phase 6 of the plan](LINUX_PORT_PLAN.md#slice-63-resume-and-recovery). In
short:

- Capture resume metadata as inert data.
- Present every saved command unchecked for explicit review.
- Revalidate identity and unchanged text immediately before execution.
- Add packaged persistence, crash, socket, SSH, upgrade, and stress harnesses.
- Add bounded scrollback sidecars only if retained in scope.

The existing state snapshot is already safe and inert; this slice must not turn
it into automatic command execution or silently restore SSH runtimes.

## Before adding any file: which category is it?

ADR 0007 splits the spike in two. Place new code before writing it.

**Durable** — survives the toolkit decision, stays C, called over FFI:
`src/gtk_key_translation.{c,h}`, `tests/gtk_key_matrix.c`,
`tests/pty_input_recorder.c`, `tests/x11_key_injector.c`.

Durable code carries no `GdkDisplay` dependency, no widget state, no I/O, and
no global state beyond the caller-owned tracker, and its expectations come from
Kitty's pinned `key_encoding.c` rather than from this host's output. Those
properties are what make it portable; breaking one needs a new ADR.

**Disposable** — replaced wholesale by the product shell:
`src/gtk_terminal_host.c`, including every `KITMUX_GTK_*` harness variable.

The test: code answering "can this toolkit do X at all?" is spike work. Code
implementing how Kitmux does X is product work.

## Do not start these

- Live macOS state import or restore. Slice 3.3's preview is read-only and
  closed; do not expand it.
- Browser product functionality. The WebKitGTK result is a coexistence probe,
  not a feature. Phase 7, and only if approved.
- Packaging. Blocked on ADR 0008 R1 and R2. Phase 8.
- The monorepo migration. It needs its own session; see
  [`docs/MONOREPO_MIGRATION.md`](docs/MONOREPO_MIGRATION.md).
- Reopening selection, clipboard, safe paste, mouse, wheel, or search. Closed
  in Slice 4.2; touch them only for a regression.

Physical-Mesa GPU proof is a real Phase 6 beta obligation, but it is a separate
gate from Slice 6.3 and does not substitute for it.

## Standing obligations

- Run `kitmux-linux/scripts/report-reference-drift.py` at every phase boundary
  and record the result in `PORT_STATUS.md`, including "no relevant drift".
- Keep `contracts/feature-inventory.json` current as Phase 6 adds behavior. It
  currently resolves 111 Linux and 234 macOS test references across 64
  features.
- ADR 0008 R1 (standalone buildability), R2 (one automated gate), and R5
  (durable mirrors for the two dependency bundles) remain open. R3 and R4
  closed on 2026-07-28.
- The open Phase 4 review findings — #3, #4, #5, #6, and #8 — are tracked in
  `PORT_STATUS.md`'s debt ledger. Clear them opportunistically when working in
  the same file.

## Resume prompt

```text
Read AGENTS.md, PORT_STATUS.md, NEXT_STEPS.md, Phase 6 in LINUX_PORT_PLAN.md,
and ADRs 0007 and 0008. Phases 0 through 5 and Slices 6.1–6.2 are closed; GTK 4
is selected, and the full terminal multiplexer alpha plus secure local control
and reviewed SSH workflows pass source and X11 gates. The clean
macOS/libkitty v0.21 reference is locked and its headless gate passes.
Implement only Slice 6.3 resume and recovery. Do not begin live macOS
import/restore, browser product work, packaging, repository migration, or new
SSH feature work; keep physical-GPU proof explicit as the separate Phase 6 beta
gate. Preserve unrelated worktree changes.
```
