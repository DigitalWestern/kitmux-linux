# Beta evidence plan: physical GPU and locked x86_64

> **For agentic workers:** execute one work stream at a time, task by task;
> every task ends with a runnable check and a commit. Record evidence in
> `PORT_STATUS.md` as dated entries, per `AGENTS.md`. This is a working
> document — delete it when both streams close.

**Goal:** close the two remaining pre-beta evidence gates that no current VM
can provide: rendering/interaction on a hardware-backed Mesa GPU, and a
locked, gated x86_64 build.

**Spec sources:** `NEXT_STEPS.md` items 1–2, `LINUX_PORT_PLAN.md` Phase 6
carry-over ("at least one physical Mesa GPU must pass before beta"),
ADR 0008 (R4/R5 and the "lock is not a support claim" rule), and the
`PORT_STATUS.md` entries dated 2026-08-18 (GPU blocked; R5 digest mismatch).

## Global constraints

- New gates take display, architecture, and paths from the environment
  (ADR 0008). Nothing added here may hard-code a host.
- Any new pinned input gets its `source-lock.json` hash in the same commit
  that introduces it (ADR 0008).
- Locking x86_64 inputs is not permission to claim x86_64 support; support
  requires a passing gate (ADR 0008).
- llvmpipe/virgl/softpipe renderer strings are correctness evidence only and
  can never close the GPU gate.
- The app's GLArea requires desktop OpenGL ≥ 3.3
  (`rust/app/src/window.rs`: `set_required_version(3, 3)`); candidate GPUs
  must have a Mesa driver exposing at least that.

---

## Work stream A — physical Mesa GPU proof

### Why the current setup cannot produce it

`lspci` in the desktop VM shows `Virtio 1.0 GPU` and `glxinfo -B` reports
llvmpipe (PORT_STATUS 2026-08-18). Lima/vz on macOS cannot pass a GPU
through. The evidence requires a Linux environment where Mesa drives real
hardware.

### Task A0: choose the host (user decision — blocks A1)

Present exactly these options; do not purchase or repartition anything
without explicit approval.

| Option | Cost | Mesa driver | GL | Notes |
| --- | --- | --- | --- | --- |
| A. Spare x86_64 PC/laptop with AMD or Intel graphics, Ubuntu 24.04 | $0 if one exists | `amdgpu` / `i915`/`xe` | 4.6 | First-class Mesa; also advances x86_64 GUI evidence. |
| B. Fedora Asahi Remix dual-boot on this Apple-silicon Mac | $0, but repartitions the user's machine | Asahi `agx` (honeykrisp) | 4.6 | ARM64 matches all existing evidence. Invasive: needs explicit consent and a backup first. |
| C. Cloud AMD-GPU instance (e.g. AWS `g4ad.xlarge`, Radeon Pro V520) | hourly | `amdgpu` | 4.6 | No hardware purchase; NVIDIA instances are excluded (proprietary driver is not Mesa; NVK is not proven for this). |
| D. ARM SBC (Raspberry Pi 5 / Rockchip) | ~$80–150 | `v3d` / `panfrost` | **3.1 — fails the 3.3 floor** | Rejected: does not meet the GL requirement. Do not buy one for this. |

Recommendation: A if such a machine exists, else C (reversible, no
repartition), else B with consent.

- [ ] **Step 1:** Ask the user which option; record the choice and the exact
  hardware (CPU arch, GPU model) in this file.

### Task A1: provision the host

- [ ] **Step 1:** Install Ubuntu 24.04 (options A/C) or Fedora Asahi Remix
  (option B) with a desktop session. For option C, use the vendor's
  Mesa-enabled image or install `xserver-xorg-video-amdgpu mesa-utils` and
  start an X session on the GPU (`sudo systemctl set-default
  graphical.target`, or `startx` on `:0`).
- [ ] **Step 2:** Clone and provision:

```sh
git clone https://github.com/DigitalWestern/kitmux-linux.git
cd kitmux-linux
kitmux-linux/scripts/provision-desktop-ubuntu.sh   # Ubuntu options
```

For Fedora Asahi, install the equivalents by hand from the script's package
list (it is apt-based); record any substitutions.

- [ ] **Step 3:** Capture the renderer proof and store it verbatim:

```sh
DISPLAY=:0 glxinfo -B | tee /tmp/gpu-proof.txt
lspci | grep -i vga   # or equivalent on ARM
```

Acceptance: `OpenGL renderer string` names the hardware driver (e.g.
`AMD Radeon ...`, `Mesa Intel(R) ...`, `Apple M-series (G13G ...)`), the
`OpenGL core profile version` is ≥ 3.3, and the string contains none of
`llvmpipe`, `softpipe`, `virgl`, `SVGA`.

### Task A2: run the rendering and interaction gates on the GPU display

Run each gate with the session's real display; `KITMUX_NOVNC_PORT=` keeps
the desktop gate off noVNC. The known menu-traversal failure in
`test-phase5-product.sh` is tracked by `MENUBAR_PLAN.md` section 8 — run
that gate last, and if slice 4 has not landed yet, record its partial result
explicitly rather than skipping it silently.

- [ ] **Step 1:** `DISPLAY=:0 KITMUX_NOVNC_PORT= kitmux-linux/scripts/test-desktop.sh`
  — expected: full X11 + nested-Wayland pass, proof PNGs regenerated.
  Warning: it changes X auto-repeat, XKB layout, and the IBus engine while
  running and restores them on exit; do not use the session interactively.
- [ ] **Step 2:** `DISPLAY=:0 kitmux-linux/scripts/test-phase4.sh` — expected:
  product lifecycle, clipboard, unsafe-paste, selection, search, font,
  mouse/wheel, foreground-close pass.
- [ ] **Step 3:** `DISPLAY=:0 kitmux-linux/scripts/test-phase4-wayland.sh` —
  expected: `GdkWaylandDisplay` asserted under nested Weston.
- [ ] **Step 4:** `DISPLAY=:0 kitmux-linux/scripts/test-phase5-navigation.sh`
  then `test-phase6-control.sh`, `test-phase6-resume.sh`,
  `test-phase6-ssh.sh` — expected: all pass unchanged.
- [ ] **Step 5:** `DISPLAY=:0 kitmux-linux/scripts/test-phase4-soak.sh` —
  expected: ≥ 1,800 s sustained, no heartbeat miss. This is the only
  performance-shaped signal a real driver adds; do not skip it.
- [ ] **Step 6:** `DISPLAY=:0 kitmux-linux/scripts/test-phase5-product.sh` —
  expected: pass once MENUBAR_PLAN section 8 has landed; otherwise record
  the exact failing check.

### Task A3: record and commit

- [ ] **Step 1:** Append a dated `PORT_STATUS.md` entry: hardware model, OS,
  the verbatim `glxinfo -B` renderer/version lines, each gate command and
  result, and the explicit non-claims (one GPU model is one data point; it
  does not prove other drivers, mixed-DPI hardware, or a touchpad).
- [ ] **Step 2:** Update `NEXT_STEPS.md` (drop item 1), `README.md` platform
  status ("runs on X11 under Mesa llvmpipe" → name the proven hardware), and
  `support-matrix.yml` if it records GPU class.
- [ ] **Step 3:** Commit: `docs: record the physical Mesa GPU gate evidence`.

---

## Work stream B — x86_64 locked bundle and gate

### Current state (audited 2026-08-27)

- `source-lock.json` still records the stale `linux-64.tar.xz` hash
  `3d0ffc61…`; the upstream rolling URL served `d1472771…` on 2026-08-18
  and the exact historical artifact is not archived anywhere we can fetch
  (the URL is neither versioned nor content-addressed — that defect is R5).
- The `dependency-bundles-v1` release hosts only `linux-arm64.tar.xz` and
  `NerdFontsSymbolsOnly.tar.xz`; `durable_dependency_platforms` is
  `["linux-arm64"]`.
- Since 2026-08-27, CI's `standalone-amd64` job passes on a native x86_64
  runner — but only via the explicitly unlocked
  `KITMUX_ALLOW_SOURCE_DEPENDENCY_BUILD=1` fallback. The runner is the
  cheapest place to gate a locked x86_64 build: no new hardware.

### Task B0: the re-lock decision (user decision — blocks B1)

Recovering `3d0ffc61…` has been attempted and failed; holding the stale
hash keeps a Tier-1 architecture permanently ungated. The decision is to
**intentionally re-lock** to the current upstream artifact, with provenance
recorded the same way the R4 entry did (derive the URL from the locked Kitty
commit `c1d507d`'s `bypy/devenv.go` + `.github/workflows/ci.py`, record the
digest and `Last-Modified`). Per ADR 0008 this changes a recorded input, so:

- [ ] **Step 1:** Get explicit user approval to re-lock `linux-64.tar.xz`.
  Do not proceed without it. If the user instead wants one more recovery
  attempt, the only untried avenue is asking upstream (kitty's maintainer or
  its CI cache) for the 2026-07-03 artifact; time-box it and return here.

### Task B1: fetch, verify, and mirror the new bundle

- [ ] **Step 1:** Download and hash:

```sh
curl -fL -o /tmp/linux-64.tar.xz \
  https://download.calibre-ebook.com/ci/kitty/linux-64.tar.xz
curl -sIL https://download.calibre-ebook.com/ci/kitty/linux-64.tar.xz \
  | grep -i last-modified
shasum -a 256 /tmp/linux-64.tar.xz
```

- [ ] **Step 2:** Sanity-check the payload before locking: list it and
  confirm it contains `bin/python`, one `lib/libpython3.*.so.1.0`, one
  `include/python3.*` directory, and `.pc` files referencing `/sw/sw`
  (the relocation in `build-kitty-dev.sh` depends on that prefix):

```sh
tar -tJf /tmp/linux-64.tar.xz | grep -E 'bin/python$|libpython3\..*\.so\.1\.0|include/python3\.[0-9]+$' 
```

- [ ] **Step 3:** Copy it to
  `kitmux-linux/locked-inputs/dependency-bundles/linux-64.tar.xz`
  (git-ignored local mirror) and upload the identical file:

```sh
gh release upload dependency-bundles-v1 \
  kitmux-linux/locked-inputs/dependency-bundles/linux-64.tar.xz \
  --repo DigitalWestern/kitmux-linux
```

### Task B2: update the lock (one commit)

- [ ] **Step 1:** In `source-lock.json`: replace the `linux-64.tar.xz` hash
  with the new digest and add `"linux-64"` to
  `durable_inputs.durable_dependency_platforms`.
- [ ] **Step 2:** Verify the pipeline accepts it exactly as CI will:

```sh
python3 kitmux-linux/scripts/release-tools.py verify-inputs --platform linux-64
```

Expected: passes against the mirrored file.

- [ ] **Step 3:** Commit lock + provenance together:
  `build: intentionally re-lock the x86_64 dependency bundle` with the
  digest, URL, and `Last-Modified` in the body. Append the matching dated
  `PORT_STATUS.md` entry in the same commit (this supersedes the R5
  "do not change the lock" hold — cite the approval).

### Task B3: flip CI's amd64 job to the locked path

- [ ] **Step 1:** In `.github/workflows/linux-standalone.yml`, delete
  `KITMUX_ALLOW_SOURCE_DEPENDENCY_BUILD: "1"` from `standalone-amd64` and
  rename the step to `Verify standalone source and headless runtime`.
- [ ] **Step 2:** Push and watch:

```sh
gh run watch "$(gh run list --repo DigitalWestern/kitmux-linux --branch main \
  --limit 1 --json databaseId -q '.[0].databaseId')" \
  --repo DigitalWestern/kitmux-linux --exit-status
```

Expected: `standalone-amd64` green on the locked path — the first real
x86_64 gate. If materialization fails, the release asset and the lock
disagree; fix the asset, never the check.

### Task B4: x86_64 package lifecycle

- [ ] **Step 1:** Read `kitmux-linux/scripts/test-package-lifecycle.sh` and
  `package-tarball.sh`/`package-deb.sh` for VM- or arch-specific
  assumptions before running them anywhere new; fix only what blocks an
  x86_64 run, per the "environment, not host" rule.
- [ ] **Step 2:** Add a `package-amd64` job to the workflow (same apt/rustup
  setup as `standalone-amd64`, `timeout-minutes: 60`) whose step runs
  `kitmux-linux/scripts/test-package-lifecycle.sh`; push and watch as in B3.
  If the script needs Podman for its fresh-container check, install
  `podman` in that job's apt list.
- [ ] **Step 3:** Expected: reproducible x86_64 tarball/`.deb` artifacts
  pass install/launch/upgrade/uninstall in the job. Record the artifact
  hashes.

### Task B5: record, claim precisely, and close

- [ ] **Step 1:** Dated `PORT_STATUS.md` entry: the re-lock provenance, the
  green locked `standalone-amd64` run URL, the package-lifecycle run URL
  and artifact hashes. State the claim exactly: **x86_64 headless source,
  runtime, and package-lifecycle gates pass in CI; x86_64 GUI/desktop
  evidence remains open** (no display in CI; the desktop gates have never
  run on x86_64). Mark R5 closed.
- [ ] **Step 2:** Update `NEXT_STEPS.md` (rewrite item 2 to the remaining
  x86_64 GUI gap, or fold it into the release-bar item) and the README
  status cell.
- [ ] **Step 3:** Commit: `docs: record the locked x86_64 CI gates; close R5`.

### Out of scope for this stream

x86_64 desktop/GUI gates. If option A or C in stream A is an x86_64 host,
running Task A2 there closes both gaps at once — prefer that sequencing.

---

## Self-review notes

- Every gate command above exists in `kitmux-linux/scripts/` today; none are
  invented. The only new automation is the workflow job in B3/B4.
- Decision gates: A0 (hardware) and B0 (re-lock) are the only steps that
  need the user; everything after each is mechanical.
- Dependency order: B1→B5 are strictly sequential; stream A is independent
  of stream B except the shared-host optimization noted in B's scope note.
