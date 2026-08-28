# Kitmux Linux terminal-alpha threat model

**Reviewed:** 2026-08-18
**Scope:** terminal-only Linux beta path: GTK app, PTY children, local control
socket, SSH profile metadata, resume metadata, and release artifacts.

This review covers the current alpha trust boundaries. It does not claim a
sandbox, remote-host security, signed distribution, or protection from a
malicious X11/Wayland compositor.

## Assets

- Commands typed into terminal sessions and their observable output.
- SSH profile metadata, approval state, agent-socket reference, and host data.
- Workspace hierarchy, cwd, inert resume metadata, and `.last-good` snapshots.
- The local control socket and the release runtime/dependency contents.

## Trust boundaries and controls

| Boundary | Threat | Current control and evidence |
| --- | --- | --- |
| Restored state to a live terminal | A snapshot silently executes an old command or reconnects SSH | Restore stores inert command/profile metadata only; review rows start unchecked; execution revalidates pane identity, command text, cwd, eligibility, and non-SSH status. `test-phase6-resume.sh` covers decline, explicit approval, identity race, SSH placeholder, upgrade, crash, and stress paths. |
| Local client to control server | Another local process reads, spoofs, floods, or blocks control traffic | Private socket path, owner/type/symlink checks, mode `0600`, Linux peer credentials, bounded frames/deadlines, and bounded event history. `test-phase6-control.sh` and the model socket suite cover these cases. |
| SSH profile/config to process launch | Shell injection, expanded private arguments, or accidental agent/session reuse | Profile files are private atomic documents; `ssh -G` output is bounded; launch uses an absolute argv with the remote command as one argument; no shell or saved SSH runtime is restored. `test-phase6-ssh.sh` covers argv and log safety. Real network/authentication is not covered. |
| PTY/libkitty diagnostics to logs | Commands, paths, titles, clipboard data, or secrets leak into diagnostics | Structured diagnostics use bounded status/byte fields and secret canaries are checked by `test-phase4.sh`; SSH acceptance checks reject private argument/stderr leakage. |
| Snapshot write/recovery | Truncation or corruption loses the last usable workspace | Atomic replacement, `.last-good` preservation, corrupt-primary recovery, bounded metadata, and SIGTERM/SIGKILL/stale-socket coverage are exercised by the Slice 6.3 gate. Power-loss durability remains unproven. |
| Release tree to installed host | Mutable developer paths or unresolved private libraries alter execution | Release closure/SBOM/hash audits, `$ORIGIN` loader checks, standalone mirror verification, deterministic tarball/`.deb` builds, and the fresh-VM lifecycle gate cover the current ARM64 path. Remote CI, x86_64, signatures, and vulnerability scanning remain open. |

## Residual risk accepted for the terminal alpha

- A same-user process with access to the display or socket can act within the
  user's desktop trust domain; no sandbox is promised.
- SSH credentials, host-key prompts, network trust, and agent signing remain
  owned by the explicit user action and the external SSH client. They are not
  simulated by the local acceptance gate.
- The current GUI evidence is ARM64 X11 with llvmpipe. Physical Mesa, native
  Wayland resume UI, x86_64, and power-loss recovery are still release gates.
- Browser panes, portals, and desktop URL integration are outside the approved
  terminal-only beta scope.

## Required follow-up before release promotion

1. **Done 2026-08-27.** The standalone workflow passed on GitHub-hosted
   runners; the run reference is recorded in `PORT_STATUS.md`.
2. Recover or intentionally re-lock the exact x86_64 dependency bundle, then
   build and test it on x86_64.
3. Run the rendering/interaction gate with a physical Mesa renderer.
4. Complete the long soak after any heartbeat failure is understood, and add
   package signing, vulnerability, and desktop-menu evidence.
