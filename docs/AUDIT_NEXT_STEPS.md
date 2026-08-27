# Audit next steps

These are the remaining decisions and proof gates after the audit remediation.

1. **Passed 2026-08-23.** The complete
   `kitmux-linux/scripts/test-all.sh` ran, including both clean Ubuntu and
   Fedora Podman release passes. The aggregate verified the new runtime output
   path and stable `share/SHA256SUMS` inventories.
2. Decide whether to recover the exact historical `linux-64.tar.xz` bundle or
   formally keep the unlocked x86_64 dependency fallback. Until then, do not claim
   x86_64 reproducibility or package support. If recovered, build and run the
   x86_64 release and package lifecycle gates.
3. Run the standalone GitHub Actions workflow and retain its result for the
   remote-CI reproducibility gate. This checkout has no configured Git remote,
   so it was not triggered locally.
4. Obtain hardware-backed Mesa rendering, then rerun the rendering and
   interaction gates on that display. Keep llvmpipe results labeled as VM
   correctness evidence.
5. **Passed in the dirty reference worktree 2026-08-23.** The complete
   macOS/libkitty C suite now passes, including the former
   `test_session.c:113` foreground-process failure. Rebaseline remains blocked
   until the relevant macOS changes are clean and reviewed.
6. Decide the release bar for real SSH/network authentication, power-loss
   recovery, desktop-menu interaction, signing, vulnerability review, full
   AT-SPI coverage, and release-maintainer ownership before promoting
   packages. Clean-target runtime launch and ARM64 package lifecycle now pass.
