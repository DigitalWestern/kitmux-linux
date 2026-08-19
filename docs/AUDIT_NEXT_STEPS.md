# Audit next steps

These are the remaining decisions and proof gates after the audit remediation.

1. Run the complete `kitmux-linux/scripts/test-all.sh`, including both clean
   Podman release passes. Confirm the new runtime output path and stable
   `share/SHA256SUMS` inventory in the aggregate result.
2. Decide whether to recover the exact historical `linux-64.tar.xz` bundle or
   formally keep the source-built x86_64 fallback. Until then, do not claim
   x86_64 reproducibility or package support. If recovered, build and run the
   x86_64 release and package lifecycle gates.
3. Run the standalone GitHub Actions workflow and retain its result for the
   remote-CI reproducibility gate.
4. Obtain hardware-backed Mesa rendering, then rerun the rendering and
   interaction gates on that display. Keep llvmpipe results labeled as VM
   correctness evidence.
5. Resolve the existing macOS `test_session.c:113` foreground-process failure
   in the dirty reference worktree, then rerun the complete libkitty C suite.
   The new child-exit pending-write regression already passes independently.
6. Decide the release bar for real SSH/network authentication, power-loss
   recovery, clean-target installation, desktop-menu interaction, signing, and
   vulnerability review before promoting packages.
