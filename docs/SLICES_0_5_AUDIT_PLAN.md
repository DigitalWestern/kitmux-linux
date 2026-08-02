# Kitmux Linux — audit remediation plan (slices 0–5)

**Written:** 2026-08-02
**Repo:** `/Users/ethanabbate/Desktop/System/home-kitmux/operating-system/linux`
**Branch:** `main`. The audit was performed at `596603b`; this plan is committed directly
on top of it, so that commit — not `596603b` — is the expected `HEAD` at preflight.
**Audience:** an implementing agent. Follow this literally. Do not improvise.

---

## 0. What is true right now (do not re-derive this)

These were verified by running the tools, not by reading docs. Trust them.

| Fact | Value | How it was checked |
| --- | --- | --- |
| Model tests | **52 pass** | `kitmux-linux/scripts/test-model.sh` on the macOS host |
| Fixtures | 6 contracts / 20 cases / 7 files | `python3 contracts/validate-fixtures.py` |
| Inventory | 97 Linux refs / 234 macOS refs / 64 features / 17 areas | `python3 contracts/validate-inventory.py` |
| Command IDs | 38 | counted in `rust/model/src/commands.rs` |
| Control methods | 44 | counted in `rust/model/src/control.rs` (`control_methods!`) |
| Runtime components | 24 | `kitmux-linux/release/runtime-components.json` |
| macOS reference | tag `macos-linux-port-baseline-2026-08-02-v0.21` → `3088295003c0842d7c3198102d0d05378da4dc62`, worktree clean | `git -C ../macos/kitmux` |
| `source-lock.json` hashes | all match on disk | re-hashed overlay patch + both `Cargo.lock` |

**Actual project state:** Phases 0–5 closed. Slice 6.1 (secure local control and CLI) is next
and has **not** been started. This plan does **not** start it.

---

## 1. Ground rules — violating any of these fails the task

1. **One task per commit.** Nine tasks below, nine commits. Do not batch.
2. **Do not start Slice 6.1.** No control socket, no CLI, no SSH, no packaging, no browser,
   no monorepo migration. This plan is maintenance only.
3. **Do not touch `../macos/kitmux`.** It is clean and tagged. Read-only.
4. **Do not edit `source-lock.json`** unless Task 1's verification tells you a hash changed.
   If a hash changed unexpectedly, **stop and report** — do not "fix" it by relocking.
5. **Do not re-run `report-reference-drift.py --relock`.** The rebaseline is already done.
6. **Rust app changes cannot be built on macOS.** `rust/app/build.rs` panics without
   `KITMUX_NATIVE_LIB_DIR`, and the crate needs GTK 4 + libkitty. All app verification
   happens inside the Ubuntu desktop VM. See §2.
7. **Never claim a gate passed that you did not run.** If you cannot run a gate, write
   "not run" in `PORT_STATUS.md`. A false evidence line is worse than a missing one.
8. **Preserve unrelated worktree changes.** Check `git status --short` before starting.
9. If a step's precondition is not met, **stop at that step** and finish the tasks that do
   not depend on it. Report exactly what you skipped and why.

---

## 2. Preflight (Task 0) — do this first, always

```sh
cd /Users/ethanabbate/Desktop/System/home-kitmux/operating-system/linux
git status --short                      # expect: empty
git log --oneline -1                    # expect: the commit that added this plan file
python3 contracts/validate-fixtures.py  # expect: 6 contracts, 20 cases, 7 versioned JSON files
python3 contracts/validate-inventory.py # expect: 97 / 234 / 64 features / inventory OK
kitmux-linux/scripts/test-model.sh      # expect: 52 tests, 0 failed
```

**If `git status --short` is non-empty:** stop. Report what is dirty. Do not proceed.

**VM state.** Both Lima VMs are currently **stopped**. Tasks 1–3 need the desktop VM:

```sh
limactl list                            # both should read Stopped
limactl start kitmux-linux-desktop      # takes ~1–2 min
limactl shell kitmux-linux-desktop -- "$PWD/kitmux-linux/scripts/start-desktop.sh"
```

Tasks 4–9 are documentation and data only. They need **no VM**. If the VM will not start,
skip to Task 4 and report Tasks 1–3 as blocked.

---

# PART A — CODE (needs the desktop VM)

---

## Task 1 — Feature-gate the safety-prompt bypass

**Severity: highest. Do this first.**

### Why

`KITMUX_AUTOPASTE` and `KITMUX_AUTOCLOSE` are read unconditionally by the release binary.
Setting `KITMUX_AUTOCLOSE=confirm` in any environment silently removes the
running-process close confirmation; `KITMUX_AUTOPASTE=confirm` removes the unsafe-paste
dialog. `docs/PHASE4_REVIEW_NOTES.md` §1 flagged this on 2026-07-28 and it is unchanged.
The confirmation code stays present and looks correct, which is what makes it dangerous.

### Files

- `kitmux-linux/rust/app/Cargo.toml`
- `kitmux-linux/rust/app/src/main.rs`
- `kitmux-linux/CMakeLists.txt`
- `kitmux-linux/scripts/build-release-runtime.sh`

### Approach — use this one, not a variant

Add a cargo feature `test-hooks`, default **off**. Guard the two decision functions with a
**runtime `cfg!` early return**, not `#[cfg]` attributes on the functions.

**Why `cfg!` and not `#[cfg]`:** `#[cfg]`-ing the function bodies out orphans
`UNSAFE_PASTE_COUNT` / `FOREGROUND_CLOSE_COUNT` (`main.rs:41–42`) and the
`std::sync::atomic::{AtomicUsize, Ordering}` import (`main.rs:31`). The gates run
`cargo clippy --locked --all-targets -- -D warnings`, so dead statics and an unused import
**fail the build**. `cfg!` avoids all of that collateral and still makes the bypass
unreachable in a default build. Do not get clever here.

### Edit 1 — `kitmux-linux/rust/app/Cargo.toml`

Append after the `[dependencies]` block (end of file):

```toml

[features]
# Enables the test-only KITMUX_AUTOPASTE / KITMUX_AUTOCLOSE modal drivers.
# OFF by default so a release build cannot have its safety prompts bypassed
# by the environment. The desktop gates enable it via -DKITMUX_APP_TEST_HOOKS=ON.
test-hooks = []
```

### Edit 2 — `kitmux-linux/rust/app/src/main.rs`

Two functions, currently at **lines 3007 and 3017**. Line numbers will drift if you edit
anything above them — match on the function signature, not the number.

Find:

```rust
fn autopaste_decision() -> Option<bool> {
    // Test-only driver for the modal path; ordinary launches leave it unset.
    match env::var("KITMUX_AUTOPASTE").as_deref() {
```

Replace with:

```rust
fn autopaste_decision() -> Option<bool> {
    // Test-only driver for the modal path; ordinary launches leave it unset.
    // Compiled inert unless the `test-hooks` feature is on, so a release build
    // cannot have its unsafe-paste confirmation removed by the environment.
    if !cfg!(feature = "test-hooks") {
        return None;
    }
    match env::var("KITMUX_AUTOPASTE").as_deref() {
```

Find:

```rust
fn autoclose_decision() -> Option<bool> {
    // Test-only driver for both branches of the foreground-process prompt.
    match env::var("KITMUX_AUTOCLOSE").as_deref() {
```

Replace with:

```rust
fn autoclose_decision() -> Option<bool> {
    // Test-only driver for both branches of the foreground-process prompt.
    // Compiled inert unless the `test-hooks` feature is on, so a release build
    // cannot have its running-process close confirmation removed by the environment.
    if !cfg!(feature = "test-hooks") {
        return None;
    }
    match env::var("KITMUX_AUTOCLOSE").as_deref() {
```

**Do not touch the three call sites** (`main.rs` ~2571, ~2958, ~3742). The signatures are
unchanged, so they compile as-is.

### Edit 3 — `kitmux-linux/CMakeLists.txt`

Add the option next to the existing ones near **line 5**:

```cmake
option(KITMUX_APP_TEST_HOOKS "Enable the app's test-only modal drivers" OFF)
```

Then in the `if(KITMUX_BUILD_APP)` block at **line 156**, the `add_custom_target(kitmux_app ...)`
currently ends its cargo line with:

```cmake
      "${CARGO_EXECUTABLE}" build --locked --release
        --manifest-path "${KITMUX_APP_MANIFEST}"
```

Introduce a variable above `add_custom_target` and append it:

```cmake
  set(KITMUX_APP_CARGO_FEATURES "")
  if(KITMUX_APP_TEST_HOOKS)
    set(KITMUX_APP_CARGO_FEATURES --features test-hooks)
  endif()
```

and change the cargo line to:

```cmake
      "${CARGO_EXECUTABLE}" build --locked --release
        --manifest-path "${KITMUX_APP_MANIFEST}"
        ${KITMUX_APP_CARGO_FEATURES}
```

**Edge case:** an empty CMake variable expands to nothing in a `COMMAND` list — this is
correct and produces no stray empty argument. Do **not** quote `${KITMUX_APP_CARGO_FEATURES}`;
quoting it turns the empty case into a literal empty argument that cargo rejects.

### Edit 4 — `kitmux-linux/scripts/build-release-runtime.sh`

In the `if [[ "${build_app}" == "1" ]]; then` block (~line 63), the `cmake_arguments+=(...)`
array at ~line 72 currently reads:

```sh
  cmake_arguments+=(
    -DKITMUX_BUILD_APP=ON
    -DKITMUX_BUILD_GTK_HOST=OFF
    -DKITMUX_PYTHON_LIBRARY_OVERRIDE="${libpython_files[0]}"
  )
```

Change to:

```sh
  cmake_arguments+=(
    -DKITMUX_BUILD_APP=ON
    -DKITMUX_BUILD_GTK_HOST=OFF
    -DKITMUX_PYTHON_LIBRARY_OVERRIDE="${libpython_files[0]}"
    -DKITMUX_APP_TEST_HOOKS="${KITMUX_APP_TEST_HOOKS:-OFF}"
  )
```

Then, in **each of these seven gate scripts**, export `KITMUX_APP_TEST_HOOKS=ON` immediately
before their `build-release-runtime.sh` invocation:

| Script | Line of the invocation |
| --- | --- |
| `kitmux-linux/scripts/test-phase4.sh` | 116 |
| `kitmux-linux/scripts/test-phase4-persistence.sh` | 109 |
| `kitmux-linux/scripts/test-phase4-programs.sh` | 61 |
| `kitmux-linux/scripts/test-phase4-soak.sh` | 68 |
| `kitmux-linux/scripts/test-phase5-navigation.sh` | 62 |
| `kitmux-linux/scripts/test-phase5-product.sh` | 121 |
| `kitmux-linux/scripts/test-phase4-clean-target.sh` | 113 |

The pattern is `KITMUX_BUILD_APP_RUNTIME=1 "${script_dir}/build-release-runtime.sh" ...`.
Add `KITMUX_APP_TEST_HOOKS=ON` alongside it on the same invocation, e.g.

```sh
KITMUX_BUILD_APP_RUNTIME=1 KITMUX_APP_TEST_HOOKS=ON \
  "${script_dir}/build-release-runtime.sh" "${runtime}"
```

`test-phase4-clean-target.sh:113` runs through `limactl shell ... env`, so it needs the var
added to that `env` list, not before it.

`test-phase4-wayland.sh` does **not** build a runtime — it wraps another gate via
`KITMUX_WAYLAND_GATE`. Leave it alone.

### Edge cases you must handle

1. **A gate script you missed.** If any gate builds a runtime without `KITMUX_APP_TEST_HOOKS=ON`,
   its `KITMUX_AUTOPASTE` / `KITMUX_AUTOCLOSE` become inert and the gate **hangs on a modal
   dialog** until its timeout, then fails. Symptom: gate times out with no error text.
   Fix: add the var to that script. Grep to confirm you got all seven:
   `grep -rln 'KITMUX_BUILD_APP_RUNTIME=1' kitmux-linux/scripts/`
2. **`Cargo.lock` must not change.** Adding a `[features]` table with no dependencies does not
   rewrite `Cargo.lock`. After building, re-verify:
   ```sh
   python3 -c "import hashlib,json;print(hashlib.sha256(open('kitmux-linux/rust/app/Cargo.lock','rb').read()).hexdigest())"
   ```
   It must equal `56dad119f0f151edcfc3df886b0cca1b6bc1f5dd675d2211107da78bdfc1249d`.
   **If it differs: stop and report.** Do not update `source-lock.json` to match — a changed
   lock means something else moved and needs a human.
3. **Clippy with `--all-features`.** If any gate runs clippy with `--all-features`, the
   `test-hooks` path is linted too. That is fine and desirable. Do not add `#[allow]`.
4. **The `cfg!` branch is not dead code.** Clippy will not warn on it. If you see a
   `clippy::` warning here, you used `#[cfg]` instead of `cfg!` — go back.
5. **Do not remove the env vars.** The gates legitimately need them. The fix is that a
   default build cannot honour them.

### Verification (must run in the desktop VM)

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase4.sh"
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase5-product.sh"
```

Both must pass. `test-phase4.sh` is the one that exercises `cancel-first` for both prompts;
`test-phase5-product.sh` is the one that uses `KITMUX_AUTOCLOSE=confirm`.

**Then prove the guard actually closes**, which is the whole point. Build one runtime with
the feature off and confirm the env var is ignored:

```sh
limactl shell kitmux-linux-desktop -- bash -lc '
  cd "'"$PWD"'" &&
  KITMUX_BUILD_APP_RUNTIME=1 kitmux-linux/scripts/build-release-runtime.sh /tmp/kitmux-nohooks &&
  strings /tmp/kitmux-nohooks/bin/kitmux | grep -c KITMUX_AUTOCLOSE'
```

The string may still appear (the `match` is compiled in, only unreachable) — that is expected
and acceptable. The behavioural proof is that with the feature off, launching with
`KITMUX_AUTOCLOSE=confirm` and a live foreground process still shows the dialog. If you have
no cheap way to assert that in the gate, **record it as "guard verified by construction, not
by gate"** in `PORT_STATUS.md`. Do not overclaim.

### Rollback

`git checkout -- kitmux-linux/rust/app/Cargo.toml kitmux-linux/rust/app/src/main.rs kitmux-linux/CMakeLists.txt kitmux-linux/scripts/`

### Commit

```
fix: compile the app's modal test drivers out of default builds

KITMUX_AUTOPASTE and KITMUX_AUTOCLOSE could remove the unsafe-paste and
running-process confirmations from a release binary. Both are now behind
the off-by-default `test-hooks` cargo feature, which the desktop gates
enable through -DKITMUX_APP_TEST_HOOKS=ON. Closes PHASE4_REVIEW_NOTES #1.
```

---

## Task 2 — Make `build.rs` relink when the C archives change

### Why

`kitmux-linux/rust/app/build.rs` emits only `cargo:rerun-if-env-changed=KITMUX_NATIVE_LIB_DIR`.
Edit `kitmux-linux/src/gtk_terminal_bridge.c`, rebuild: CMake produces a fresh
`libkitmux_terminal_bridge.a`, Cargo sees no changed input, skips the link, and the binary
keeps the previous C code. That file grew 185 → 238 lines during Slice 5.2's multi-region
renderer, so this was live during the slice. Failure mode is "my fix did nothing",
which is expensive to diagnose. `docs/PHASE4_REVIEW_NOTES.md` §2.

### Edit — `kitmux-linux/rust/app/build.rs`

Current file is 13 lines. Insert before the final `rerun-if-env-changed` line:

```rust
    println!("cargo:rerun-if-changed={native}/libkitmux_terminal_bridge.a");
    println!("cargo:rerun-if-changed={native}/libkitmux_key_translation.a");
```

### Edge cases

1. **First configure, archives absent.** `cargo:rerun-if-changed` pointing at a path that
   does not exist yet is legal — Cargo treats "missing" as a state and reruns when it
   appears. It does **not** error. Do not add an existence check.
2. **Do not add `rerun-if-changed` for the `.c` sources.** Cargo does not build them; CMake
   does. Watching the `.c` file would rerun `build.rs` before the archive was rebuilt, which
   is the wrong ordering. Watch the archives only.
3. **Do not remove `rerun-if-env-changed`.** It is still needed.
4. Emitting **any** `rerun-if-*` already suppresses Cargo's default "rerun on any source
   change" behaviour — that was already true before this change, so it introduces no
   regression.

### Verification

In the desktop VM, prove the relink now happens:

```sh
limactl shell kitmux-linux-desktop -- bash -lc '
  cd "'"$PWD"'" &&
  KITMUX_BUILD_APP_RUNTIME=1 KITMUX_APP_TEST_HOOKS=ON \
    kitmux-linux/scripts/build-release-runtime.sh /tmp/kitmux-relink-a &&
  touch kitmux-linux/src/gtk_terminal_bridge.c &&
  KITMUX_BUILD_APP_RUNTIME=1 KITMUX_APP_TEST_HOOKS=ON \
    kitmux-linux/scripts/build-release-runtime.sh /tmp/kitmux-relink-b'
```

Each `build-release-runtime.sh` uses a fresh build dir, so this test alone does not prove
incrementality. The honest check is simpler: confirm the two `rerun-if-changed` lines appear
in the build script output, then run the normal gate:

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase4.sh"
```

If you cannot construct an incremental-rebuild assertion cheaply, say so. Do not fake one.

### Commit

```
build: relink the app when its C archives change

build.rs watched only KITMUX_NATIVE_LIB_DIR, so editing
src/gtk_terminal_bridge.c rebuilt the static archive without relinking the
binary. Closes PHASE4_REVIEW_NOTES #2.
```

---

## Task 3 — Clear the PTY source id on the missing-session path

### Why

`kitmux-linux/rust/app/src/main.rs`, `pump_pty` (~line 1989). At ~line 2001:

```rust
        let Some(session) = terminal.sessions.get_mut(&surface_id) else {
            return 0;
```

Returning `0` tells GLib to drop the source, but `session.pty_source` is never zeroed —
there is no session to zero it on. A later `shutdown` iterates `self.sessions` and calls
`g_source_remove(session.pty_source)` on ids GLib has already dropped, which is a GLib
runtime critical. `docs/PHASE4_REVIEW_NOTES.md` §7.

### Assess before editing

Read `pump_pty` in full (roughly lines 1989–2100) and `shutdown` (roughly 1899–1958).
Determine whether the missing-session path is reachable at all in the current registry
design — surfaces are removed from `sessions` when their model surface closes, and the
source is removed at the same time in the close path.

**If you conclude it is unreachable**, do not add speculative code. Instead:

- add a one-line comment at the `return 0` naming why it cannot happen, and
- record in Task 8's debt ledger that #7 was assessed as unreachable, with the reasoning.

**If it is reachable**, the fix is to make `shutdown` tolerant rather than to chase every
early return. In `shutdown`, the loop currently reads:

```rust
            if session.pty_source != 0 {
                unsafe { g_source_remove(session.pty_source) };
                session.pty_source = 0;
            }
```

`g_source_remove` on a stale id is exactly the critical being avoided. Prefer holding the
`glib::SourceId` and calling `.remove()`, or track removal in a `HashSet<u32>` of live ids.
**Do not** wrap it in a bare `unsafe` suppression.

### Edge cases

1. Any change to `shutdown`'s bookkeeping can alter the `sessions=N reaped=B` diagnostic
   line. `test-phase5-product.sh:113` and `test-phase5-navigation.sh:273` both grep
   `^kitmux event=shutdown .* sessions=${expected_sessions} reaped=true$`. If your change
   moves `N`, those gates fail. Re-run both.
2. `shutdown` is called twice per window close (`connect_close_request` then
   `connect_unrealize`) — see Task 8 #5. The second call currently emits `sessions=0
   reaped=true`, which cannot match a gate expecting N>0. Do not "fix" the double call as
   part of this task; it is tracked separately.

### Verification

```sh
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 \
  "$PWD/kitmux-linux/scripts/test-phase5-product.sh"
limactl shell kitmux-linux-desktop -- env DISPLAY=:1 KITMUX_RAPID_NAV_GATE=1 \
  "$PWD/kitmux-linux/scripts/test-phase5-navigation.sh"
```

Both must pass, and neither log may contain `GLib-CRITICAL`.

### Commit

```
fix: do not remove an already-dropped PTY source id
```

(or, if unreachable: `docs: record why pump_pty's missing-session path is unreachable`)

---

# PART B — DOCUMENTATION AND DATA (no VM needed)

These are independent of Part A. If the VM is unavailable, do all of these anyway.

---

## Task 4 — Fix `PORT_STATUS.md`'s self-contradiction and stale handoff

### Why

The evidence ledger disagrees with itself about which phase is closed, and its final section
— the last thing a handoff agent reads — points at already-completed work.

### Edit 4a — header, `PORT_STATUS.md` line 5

Find:

```
**Implementation state:** Phases 0 through 4 are closed. GTK 4
```

Replace with:

```
**Implementation state:** Phases 0 through 5 are closed. GTK 4
```

### Edit 4b — the handoff block, `PORT_STATUS.md` lines 1317–1331

Find the whole `## Next-agent handoff` section and replace its body with:

```markdown
## Next-agent handoff

Begin [Slice 6.1 in the implementation plan](LINUX_PORT_PLAN.md#slice-61-secure-local-control-and-cli)
using the exact sequence in [`NEXT_STEPS.md`](NEXT_STEPS.md). Phases 0 through 5 are
closed; GTK 4 is selected, and the release-shaped terminal multiplexer alpha has
hierarchy navigation, nested splits, one permanent session per live surface, native
command/settings controls, full safe hierarchy persistence, and close-chain foreground
review. The macOS/libkitty v0.21 reference is locked and its Ubuntu headless gate passes.

Phase 3 is closed through Slice 3.3. Do not expand its preview into a live state import,
shell restore, SSH launcher, or persistence writer before those product paths reach their
assigned later phases.

Implement only Slice 6.1: private XDG runtime directory and `0600` socket, owner/type/
symlink checks and Linux peer credentials, bounded frames/clients/reads/writes/timeouts/
event history, explicit multiple-instance behavior, and a package-managed CLI with a
diagnosable user-local fallback. Do not begin live macOS import/restore, browser product
functionality, packaging, or repository migration. Physical-Mesa GPU proof remains a
separate Phase 6 beta obligation.
```

### Edge cases

1. **Verify the anchor.** `LINUX_PORT_PLAN.md:738` is `#### Slice 6.1: Secure local control and CLI`,
   so the GitHub anchor is `#slice-61-secure-local-control-and-cli`. Confirm by reading that
   heading before writing the link.
2. **Do not rewrite historical evidence entries.** Everything under `## Evidence log` is a
   dated record. Entries that were accurate when written stay as-is, including old test
   counts like "48 tests" in the 2026-07-28 Phase 4 entry and "75 Linux / 234 macOS" at
   line 265. Those are point-in-time facts, not errors.
3. Do **not** touch line 233 ("80 resolving Linux inventory references") for the same reason.

### Verification

```sh
grep -n 'Phases 0 through' PORT_STATUS.md   # every hit must say "0 through 5"
grep -n 'Slice 5.3' PORT_STATUS.md          # must only appear in dated evidence entries
```

### Commit

```
docs: correct the PORT_STATUS phase state and next-agent handoff
```

---

## Task 5 — Bring `README.md` up to the real state

### Why

Last touched 2026-07-25. It is the public-facing file and it still describes GTK as an
undecided candidate, five slices before reality.

### Edit 5a — the Linux row, `README.md` line 17

Replace the entire `| Linux | ... |` cell with:

```
| Linux | Experimental, GPL-3.0-only. Phases 0 through 5 are complete: the engine passes headless and clean-runtime gates; GTK 4 is the selected toolkit; and a release-shaped terminal multiplexer alpha runs on X11 and native Wayland with Mesa llvmpipe — hierarchy navigation, nested splits, one permanent libkitty session per live surface, selection/clipboard/paste-safety/mouse/wheel/search, native command palette and settings, full safe hierarchy persistence, and close-chain foreground review. x86_64, physical-GPU and mixed-DPI hardware behavior, complete AT-SPI coverage, browser product behavior, and native packaging remain unproven. This repository cannot currently be built standalone — see ADR 0008 R1. |
```

### Edit 5b — `README.md` line 20

Find:

```
Linux is not yet a production application or downloadable desktop package.
GTK is the leading toolkit candidate, not a final selection.
```

Replace with:

```
Linux is not yet a production application or downloadable desktop package.
GTK 4 was selected in Slice 2.3 on 2026-07-26 after every Phase 2 kill test passed;
see ADR 0002.
```

### Edit 5c — the architecture diagram, `README.md` line 26

Find `Linux candidate: GTK 4` → replace with `Linux: GTK 4`.

### Edit 5d — the roadmap, `README.md` lines 78–86

Replace the five numbered items with:

```markdown
1. Add secure local control and a CLI (Slice 6.1), then SSH and agent workflows,
   then resume and recovery — Phase 6.
2. Prove one physical Mesa GPU before beta; llvmpipe proves correctness, not drivers.
3. Close ADR 0008 R1 and R2 — standalone buildability and one automated gate — before
   any packaging work or a second contributor.
4. Produce supported packages only after R1 and R2 close — Phase 8.
5. Consider browser panes only after the terminal product is stable — Phase 7.
```

### Edge cases

1. The macOS row (line 16) is **not** in scope. Do not edit it; this repo is not its source
   of truth.
2. Keep the "Build and test" command block as-is. Those commands are still correct.
3. Do not add a Phase 5 evidence dump to the README. Numbers belong in `PORT_STATUS.md`;
   the README links there.

### Verification

```sh
grep -n -i 'candidate' README.md   # must return no line describing GTK as undecided
```

### Commit

```
docs: update the README to the closed Phase 5 state
```

---

## Task 6 — Fix `AGENTS.md`'s stale toolkit instruction

### Why

`AGENTS.md` is the agent entry point. Line 36 instructs the next agent to treat a closed
decision as open.

### Edit — `AGENTS.md` lines 36–37

Find:

```
- Treat GTK 4 as a candidate until the complete Slice 2.3 decision gate
  passes. Slice 2.1 alone did not select it.
```

Replace with:

```
- GTK 4 is the selected Linux toolkit (Slice 2.3, 2026-07-26; ADR 0002). Do not
  reopen the toolkit question or start a Qt probe without a new ADR.
```

### Edge cases

1. Every other working rule in `AGENTS.md` is still correct. Change only this bullet.
2. Leave the ADR 0007 durable/disposable bullet (lines 38–40) untouched — still accurate.
3. Leave the "Read this first" ordering untouched.

### Verification

```sh
grep -n 'candidate' AGENTS.md   # expect no hits
```

### Commit

```
docs: record the closed GTK toolkit decision in the agent instructions
```

---

## Task 7 — Bring `docs/LINUX_DEVELOPMENT.md` current through Slice 5.3

### Why

README and `PORT_STATUS.md` both name this as *the* operational reference. Its
"What is proven" section stops mid-Slice 5.1 and states a test count that is wrong.

### Edit 7a — `docs/LINUX_DEVELOPMENT.md` lines 35–39

Find:

```
- The display-free Rust model gate passes 48 model/contract/interaction/
  persistence tests on
  macOS and Ubuntu ARM64.
```

Replace `48` with `52`. **Verify first** by running `kitmux-linux/scripts/test-model.sh`
and reading the summed test count off the output — do not trust this document, including
this plan.

### Edit 7b — `docs/LINUX_DEVELOPMENT.md` lines 57–62

Replace the entire Slice 5.1 bullet:

```
- Slice 5.1's display-free navigation foundation passes on macOS and Ubuntu
  ARM64: bounded workspace/group/tab naming, stable-ID reorder and explicit
  non-empty closes, stable-command navigation overrides, and separate
  Super+number workspace versus Alt+number terminal-tab namespaces. The GTK
  app compiles against this model; product sidebar/tab-strip behavior is not
  yet GUI-proven.
```

with:

```
- Phase 5 is closed. Slice 5.1's hierarchy — bounded workspace/group/tab naming,
  stable-ID reorder, explicit non-empty closes, and separate Super+number workspace
  versus Alt+number terminal-tab namespaces — is proven display-free on macOS and
  Ubuntu ARM64 and GUI-proven through a responsive GTK sidebar, group row, and tab
  row on X11 and native Wayland.
- Slice 5.2 gives every live terminal surface a permanent libkitty child and
  idle-priority GLib PTY source keyed by stable SurfaceId. One GTK GL area renders
  the active tab's nested split leaves through a scissored multi-region C bridge;
  inactive tabs stay owned and keep draining without entering layout or draw.
  Pointer divider drag, pointer/cycle/directional focus, and Super-based keyboard
  resize pass focused X11 and native-Wayland gates from fresh release runtimes.
- Slice 5.3 adds a native command palette over the frozen 38-ID catalog and a native
  settings dialog over the bounded settings document. State round-trips the complete
  hierarchy, nested ratios, schema-supported IDs, names/titles, selection, surface
  stacks, and safe per-surface cwd into fresh passwd shells without executing saved
  resume text; a corrupt primary recovers the last-good hierarchy. Pane, group,
  workspace, and window closes share one scoped live foreground recheck, and native
  terminal/button roles with terminal → Commands → Settings → terminal focus transfer
  pass on both backends.
```

### Edit 7c — "What is not proven", after line 71

The existing bullet says accessibility is unproven, which is now too strong. Replace the
accessibility clause so it reads:

```
- Complete AT-SPI screen-reader and terminal-content coverage. Slice 5.3 proves native
  roles, labels, and keyboard-only focus order; it does not prove a screen reader.
```

### Edge cases

1. Sections below line 82 (`## Report reference drift`, `## Materialize the locked source`,
   `## Headless VM`, `## Desktop VM and GTK gate`, `## Release runtime…`) are **operational
   command references** and are still correct. Do not touch them.
2. Line 277's "Run the Phase 4 shell/editor matrix and required 30-minute interaction soak"
   is still a real, current command. Leave it.
3. Do not add Slice 5 gate commands to this file unless you have run them. If you add them,
   they must match the exact invocations recorded in `PORT_STATUS.md` lines 141–157.

### Verification

```sh
grep -n '48 model' docs/LINUX_DEVELOPMENT.md    # expect no hits
grep -n 'not yet GUI-proven' docs/LINUX_DEVELOPMENT.md  # expect no hits
```

### Commit

```
docs: bring the Linux development guide current through Slice 5.3
```

---

## Task 8 — Small factual corrections across three files

Three unrelated one-line errors. One commit is fine.

### 8a — `NEXT_STEPS.md` line 280

Find:

```
- `contracts/feature-inventory.json` now has per-behavior status and 75
  resolving Linux test references. Keep it current as Phase 5 adds behavior.
```

Replace with:

```
- `contracts/feature-inventory.json` now has per-behavior status and 97
  resolving Linux test references. Keep it current as Phase 6 adds behavior.
```

**Verify 97 first** with `python3 contracts/validate-inventory.py`. If it prints a different
number, use that number.

### 8b — `docs/decisions/0008-reproducibility-and-gating.md` line 30

Find:

```
`patches/` from it. `kitmux-linux/patches/` is an empty directory in this tree.
```

Replace with:

```
`patches/` from it. `kitmux-linux/patches/` now holds only the hash-locked Linux
render-scale overlay added in Slice 2.2E; the authoritative libkitty glue and Kitty
patches still come from the macOS repository.
```

**Why:** `kitmux-linux/patches/libkitty/0001-render-scale.patch` is tracked and hash-locked
in `source-lock.json` under `linux_overlays`. `PORT_STATUS.md`'s copy of R1 (line 1276) was
already corrected; the ADR was not. Do **not** change R1's status — it is still open, and
the overlay does not make the tree standalone-buildable.

### 8c — `support-matrix.yml` line 175

Find:

```
    limitation: semantic role/focus viability only; full AT-SPI screen-reader and terminal-content coverage is Phase 5
```

Replace with:

```
    limitation: semantic role/focus viability only; Slice 5.3 added native roles, labels, and keyboard-only focus order, but full AT-SPI screen-reader and terminal-content coverage remains a Phase 9 accessibility-review obligation
```

Also bump the file header (line 2) `updated: 2026-08-01` → `updated: <today>`.

### Edge cases

1. `support-matrix.yml` must stay valid YAML. After editing:
   `python3 -c "import yaml,sys; yaml.safe_load(open('support-matrix.yml'))"`
   If PyYAML is unavailable, use `limactl validate` on the lima files as a proxy only —
   it does not cover this file. Prefer installing nothing; visually confirm the `limitation:`
   value stays on one line and contains no unquoted `:` followed by a space.
   **The replacement text above contains `; ` and `,` only — no `: ` — which is why it is
   safe unquoted. If you rewrite it, keep that property or quote the whole scalar.**
2. Do not renumber or reclassify anything else in `support-matrix.yml`.

### Verification

```sh
python3 contracts/validate-inventory.py
python3 -c "import yaml;yaml.safe_load(open('support-matrix.yml'));print('yaml ok')"
```

### Commit

```
docs: correct inventory reference count, ADR 0008 R1 scope, and the AT-SPI phase pointer
```

---

## Task 9 — Backfill missing Phase 0 evidence and the Slice 4.1 inventory gap

Two data gaps. One commit.

### 9a — Phase 0 evidence entries

`LINUX_PORT_PLAN.md:108` states: *"A phase is complete only when its exit gate has evidence
in `PORT_STATUS.md`."* Phase 0 is marked complete through Slice 0.6, but **Slices 0.2 and
0.3 have no dated evidence entry** in the ledger. They are also the only Phase-0 slices in
the plan without a `— complete` marker on their heading.

Add **one** entry to `PORT_STATUS.md`, inserted in reverse-chronological position — that is,
immediately **before** the `### 2026-07-23 — planning review` entry at line 1216, since these
slices predate everything else.

Template (fill in only what you can verify):

```markdown
### 2026-07-23 — Slices 0.2 and 0.3 backfilled

- Backfill entry written 2026-08-02. Slices 0.2 and 0.3 were completed during the
  original planning pass but never received their own dated ledger entry, so Phase 0's
  closure rested on artifacts rather than on recorded evidence. This entry records the
  artifacts; it produces no new behavioral, GUI, package, or clean-machine evidence.
- Slice 0.2 — parity inventory: `contracts/feature-inventory.json` exists with 64
  features across 17 areas. Every row carries a stable ID, behavior, macOS
  source/test references, a classification, a Linux acceptance statement,
  dependencies, translation notes, and a `linux_status`. Verified by
  `python3 contracts/validate-inventory.py`, which resolves 97 Linux test references
  and 234 macOS source and test references at
  `macos-linux-port-baseline-2026-08-02-v0.21` and reports `inventory OK`.
  The 2026-07-25 plan audit found the original 16 rows too coarse for Phase 9's parity
  gate; decomposition to per-behavior rows was completed before Phase 5.
- Slice 0.3 — decisions and support targets: ADRs 0001 through 0005 exist under
  `docs/decisions/` and cover the Rust host, the GTK kill spike, repository and engine
  source, contract versioning, and Python/packaging. `support-matrix.yml` exists and
  records OS images, architecture, desktop, display backend, GPU class, compiler,
  Python, toolkit, and package targets. ADRs 0006, 0007, and 0008 were added later by
  the 2026-07-25 plan audit.
- Source-tested: inventory and ADR/matrix structure only. GUI-tested,
  package-tested, and clean-machine-tested: none. No slice behavior was re-run to
  produce this entry.
```

**Before writing it, verify every claim in it.** Run the validator. Count the ADR files.
Confirm `support-matrix.yml` actually contains those fields. If any claim does not hold,
delete that sentence rather than softening it.

Then update `LINUX_PORT_PLAN.md` headings at lines 165 and 178 to match the 0.4/0.5/0.6
convention:

- `#### Slice 0.2: Build the parity inventory` → `#### Slice 0.2: Build the parity inventory — complete`
- `#### Slice 0.3: Record decisions and support targets` → `#### Slice 0.3: Record decisions and support targets — complete`

**Do not** add `— complete` to Slice 0.1. It is covered by the
`### 2026-07-23 — macOS baseline freeze` entry, but confirm that entry actually satisfies
0.1's gate list (the seven `make` commands at plan lines 151–159) before deciding. If it
does, add `— complete` to 0.1 too and say so in the commit message. If it does not, leave
it and note the gap in the debt ledger (9c).

### 9b — Slice 4.1 inventory attribution

No `linux_status` field in `contracts/feature-inventory.json` references Slice 4.1. Every
other product slice is attributed: 4.2 → 13 rows, 4.3 → 4, 5.1 → 4, 5.2 → 4, 5.3 → 10.
`AGENTS.md` step 3 of "Finishing a slice" requires the inventory be updated per slice.

Slice 4.1 delivered: one window/engine/terminal-widget/session; XDG config/state/cache/data/
runtime paths; account shell lookup and documented desktop-launch environment; PTY pumping,
redraw scheduling, title/cwd updates, child exit, ordered shutdown; and structured
diagnostics that omit secrets.

Candidate rows to extend (**verify each against `PORT_STATUS.md` lines 379–413 before
editing** — that is the Slice 4.1 evidence entry, and it is the only thing you may cite):

| Row | What Slice 4.1 adds to its status |
| --- | --- |
| `engine.init-shutdown` | product-app engine init and ordered shutdown in the release runtime |
| `engine.session-create-close` | one passwd-shell session created and its child reaped on exit |
| `terminal.title-and-bell` | live title and cwd updates in the product shell |
| `render.init-failure-visible` | the app's in-window "Terminal unavailable" path |

**Append** to the existing `linux_status` strings — never replace prior evidence. Example
shape (match the existing punctuation style, which is `; Slice N.M ...`):

```
"linux_status": "<existing text>; Slice 4.1 proves the same path in the release-shaped
product app on Ubuntu 26.04 ARM64 X11"
```

### Edge cases

1. `validate-inventory.py` resolves every path in `macos_sources` / `macos_tests` against the
   tagged macOS checkout. **Do not add references to files that do not exist at that tag** —
   the validator will fail. If you want to cite a Linux test, put it where the existing rows
   put Linux tests, and re-run the validator.
2. The Linux-reference count will change if you add Linux test references. That is expected.
   **After editing, re-run the validator and propagate the new number to `NEXT_STEPS.md`
   line 280** (Task 8a). Do this in the same commit so the two never disagree.
3. `contracts/feature-inventory.json` must stay valid JSON and keep its key order stable
   where practical. Edit with a text editor, not by round-tripping through `json.dump`,
   which will reorder and reformat the whole file and produce an unreviewable diff.
4. Do **not** change any row's `classification`. Reclassification is a Phase 9 decision.
5. Bump the file's `updated` field to today.

### 9c — Debt ledger for the remaining review findings

`docs/PHASE4_REVIEW_NOTES.md` exists but nothing links to it, so its findings are invisible
to the next slice. Add a subsection to `PORT_STATUS.md` under `## Current blockers and
limits`, immediately before `### Reproducibility defects (ADR 0008)` at line 1271:

```markdown
### Carried-forward Phase 4 review findings

`docs/PHASE4_REVIEW_NOTES.md` recorded eight findings on 2026-07-28 and none were fixed
before Phase 5 closed. Status as of 2026-08-02:

- **#1 environment-disableable safety prompts — closed.** `KITMUX_AUTOPASTE` and
  `KITMUX_AUTOCLOSE` are now behind the off-by-default `test-hooks` cargo feature.
- **#2 build.rs did not relink on C changes — closed.** `build.rs` now emits
  `cargo:rerun-if-changed` for both static archives.
- **#7 dangling event-source id — <closed | assessed unreachable>.** <reasoning>
- **#3 two exported bridge functions have no caller — open.**
  `kitmux_widget_surface_scale` and `kitmux_session_draw_preserving_gl_state` in
  `src/gtk_terminal_bridge.c`. The review verified this is not a scaling bug. Delete
  them or record why they are kept.
- **#4 `let _ = committed;` — open.** `rust/app/src/main.rs` in the key-pressed handler.
  Either the commit state should affect whether the release is withheld, or this is a
  leftover.
- **#5 shutdown runs and logs twice — open, mitigated.** `connect_close_request` and
  `connect_unrealize` both call `shutdown`. The Phase 5 gates assert
  `sessions=N reaped=true` with N greater than zero, and the second call emits
  `sessions=0`, so it cannot satisfy them. The masking risk the review described is
  gated against, not fixed in code.
- **#6 libkitty's error text is discarded on startup failure — open.**
  `initialize` fills a 1024-byte buffer and returns a stage name without reading it.
  `render.init-failure-visible` is also one of the inventory rows with no macOS test,
  so no gate on either platform pins this.
- **#8 wheel speed is an unnamed constant — open.** `-dy * cell_points * 5.0`; kitty's
  own default is three. It belongs in settings.

Six inventory rows carry no `macos_tests`, five of them terminal-alpha:
`render.gl-state-isolation`, `render.scale-correctness`, `render.init-failure-visible`,
`render.webkit-coexistence`, and `keyboard.press-release-repeat`. Phase 9's parity gate
resolves rows against macOS behavior; these have no macOS oracle.
```

**Fill in #1, #2, and #7 with what actually happened.** If Part A was skipped because the
VM was unavailable, mark #1 and #2 **open** and say the VM was unavailable. Do not write
"closed" for work you did not do.

### Verification

```sh
python3 contracts/validate-inventory.py     # must print inventory OK
python3 contracts/validate-fixtures.py      # must still print portable fixtures OK
python3 -c "import json;json.load(open('contracts/feature-inventory.json'));print('json ok')"
```

### Commit

```
docs: backfill Phase 0 evidence, Slice 4.1 inventory rows, and the review debt ledger
```

---

## Task 10 — Housekeeping

### Why

Four empty `.phase4-clean-target.*` directory skeletons sit in the repo root, dated
2026-07-29, leaked by `test-phase4-clean-target.sh:106`, which does
`mktemp -d "${linux_root}/.phase4-clean-target.XXXXXX"` — inside the repository. Its
cleanup trap ran, but the directory shells survived.

They are invisible to `git status` **only** because they contain no files. If a future run
leaks one containing a file, `report-reference-drift.py --relock` refuses to run
(its Linux clean-worktree guard, line 241) for a reason with no connection to rebaselining.

### Edits

1. Remove the leaked directories:
   ```sh
   rmdir -p .phase4-clean-target.*/runtime/* 2>/dev/null
   rm -rf .phase4-clean-target.*
   ```
   **Before running `rm -rf`, confirm they are still empty:**
   ```sh
   find .phase4-clean-target.* -type f | wc -l    # must print 0
   ```
   If it prints anything other than 0, **stop** — a gate may be running. Do not delete.

2. Add to `.gitignore`, after the existing `kitmux-linux/vendor/` line:
   ```
   .phase4-clean-target.*/
   ```

### Edge cases

1. **Do not** change `test-phase4-clean-target.sh` to use `/tmp` instead. The script places
   the staging root inside `${linux_root}` because that is the only path mounted writable
   into the Lima VM. Moving it breaks the gate. The `.gitignore` entry is the correct fix.
2. Do not delete these while a Phase 4/5 gate is running — you would delete a live runtime.
   Check `limactl list` shows the VM stopped, or that no gate is in flight.

### Verification

```sh
ls -d .phase4-clean-target.* 2>/dev/null   # expect: no matches
git status --short                          # expect: only .gitignore modified
```

### Commit

```
chore: ignore and clear leaked clean-target staging directories
```

---

## 3. Final verification matrix

Run everything after the last commit. Record each result verbatim in `PORT_STATUS.md`.

| # | Command | Expected | Needs VM |
| --- | --- | --- | --- |
| 1 | `git status --short` | empty | no |
| 2 | `python3 contracts/validate-fixtures.py` | `6 contracts, 20 cases, 7 versioned JSON files` / `portable fixtures OK` | no |
| 3 | `python3 contracts/validate-inventory.py` | `inventory OK`, 234 macOS refs, 64 features | no |
| 4 | `kitmux-linux/scripts/test-model.sh` | 52 tests, 0 failed, fmt + clippy clean | no |
| 5 | `kitmux-linux/scripts/test-phase3.sh` | 52 Rust tests, 6/20/7 fixtures, 9 macOS consumer tests | no (needs macOS repo) |
| 6 | `kitmux-linux/scripts/materialize-reference.sh` | 7 reference files + 1 Linux overlay verified | no |
| 7 | `kitmux-linux/scripts/report-reference-drift.py` | no mandatory drift | no |
| 8 | `limactl shell kitmux-linux -- "$PWD/kitmux-linux/scripts/test-headless.sh"` | 6 C/C++/ELF/engine/session/stress tests | headless VM |
| 9 | `limactl shell kitmux-linux-desktop -- env DISPLAY=:1 "$PWD/kitmux-linux/scripts/test-phase4.sh"` | pass | desktop VM |
| 10 | `limactl shell kitmux-linux-desktop -- env DISPLAY=:1 "$PWD/kitmux-linux/scripts/test-phase5-product.sh"` | pass | desktop VM |
| 11 | `limactl shell kitmux-linux-desktop -- env DISPLAY=:1 KITMUX_RAPID_NAV_GATE=1 "$PWD/kitmux-linux/scripts/test-phase5-navigation.sh"` | pass | desktop VM |
| 12 | `limactl shell kitmux-linux-desktop -- env DISPLAY=:1 "$PWD/kitmux-linux/scripts/test-phase4-wayland.sh"` | pass, asserts `GdkWaylandDisplay` | desktop VM |

Then add **one** dated entry to `PORT_STATUS.md`'s evidence log, above the 2026-08-02
rebaseline entry, following the existing format exactly — including the closing
`Source-tested: / GUI-tested: / Release-layout-tested: / Native-package-tested…: none`
lines and an explicit "Slice 6.1 is next; no Phase 6 product work was started."

**If gates 9–12 were not run**, the entry must say so plainly: *"GUI-tested: not run; the
desktop VM was unavailable and Tasks 1–3 were not performed."*

---

## 4. Stop rules

Stop and report instead of proceeding if any of these happen:

- `git status --short` is non-empty at preflight.
- Any `source-lock.json` hash stops matching its file.
- `../macos/kitmux` is dirty, or its HEAD is not `3088295003c0842d7c3198102d0d05378da4dc62`.
- `validate-inventory.py` or `validate-fixtures.py` starts failing after an edit you made.
- A desktop gate hangs rather than fails — that is the Task 1 edge case #1 signature.
- You find yourself editing `kitmux-linux/rust/model/` for any task in this plan. None of
  these tasks touch the model crate.
- You are about to write an evidence line for a command you did not run.

## 5. Explicitly out of scope

Do not do any of these, even if they look easy:

- Slice 6.1 itself — control socket, CLI, peer credentials, framing.
- ADR 0008 R1 / R2 / R5 (standalone build, CI, bundle mirroring).
- The monorepo migration.
- Physical-GPU or x86_64 work.
- Browser or packaging work.
- Re-running `--relock`, re-tagging macOS, or regenerating the fixture corpus.
- Phase 4 review findings #3, #4, #5, #6, #8 — they get **recorded** in Task 9c, not fixed.
  Fixing them changes product behavior mid-audit, which is how an audit becomes a rewrite.
