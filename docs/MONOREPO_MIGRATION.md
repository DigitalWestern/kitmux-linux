# Monorepo migration proposal

Status: proposed, not executed.

Move the macOS and Linux implementations into one private GitHub repository
when there is a fresh work window for migration and review. A monorepo is the
practical default because the platforms already depend on one `libkitty`
contract, the same behavioral fixtures, and coordinated release/compliance
work. There is no current technical reason to make routine cross-platform
changes span multiple repositories.

Do not combine histories, move files, or extract shared code as part of an
ordinary feature slice.

## Recommended destination

```text
kitmux/
├── README.md
├── AGENTS.md
├── Makefile
├── contracts/
│   ├── schemas/
│   ├── fixtures/
│   │   └── v1/
│   └── feature-inventory.json
├── engine/
│   └── libkitty/
│       ├── include/
│       ├── src/
│       ├── py/
│       ├── tests/
│       └── patches/
├── platforms/
│   ├── macos/
│   │   ├── app/
│   │   ├── packaging/
│   │   └── docs/
│   └── linux/
│       ├── app/
│       ├── packaging/
│       ├── vm/
│       └── docs/
├── docs/
│   ├── architecture/
│   ├── decisions/
│   ├── roadmap/
│   └── handoff/
└── scripts/
    ├── macos/
    └── linux/
```

The first history-preserving import should be deliberately less tidy:
`platforms/macos/` should initially contain the complete current macOS tree,
and `platforms/linux/` the complete current Linux tree. Extract `engine/` and
root `contracts/` only in later reviewable commits after both imported
histories and gates are intact. This avoids mixing a history migration with an
architecture refactor.

## Shared ownership

Root `contracts/` should own:

- versioned valid and invalid fixtures;
- schemas and bounds;
- stable command identifiers and error codes;
- the cross-platform feature inventory;
- producer/consumer compatibility tests.

Root `engine/libkitty/` should eventually be the one authoritative C/Python
engine source. Platform hosts may own adapters and build/package logic, but
must not maintain copied engine implementations.

Do not force the Swift/AppKit product model into a shared language. Share
source only where it clearly reduces maintenance; share observable behavior
through contracts first.

## Preserve both Git histories

Perform the migration in disposable clones:

1. Freeze and tag clean tips in both source repositories.
2. Mirror-clone each source so the originals remain untouched.
3. Use `git filter-repo --to-subdirectory-filter platforms/macos` on the macOS
   clone and the equivalent `platforms/linux` transform on the Linux clone.
4. Create a new private destination repository and merge the rewritten roots
   with `--allow-unrelated-histories`.
5. Import and rename nonconflicting historical tags before pushing.
6. Compare commit counts, tags, source hashes, and `git log --follow` on
   representative files.
7. Run both platform gates before changing directory organization further.

`git filter-repo` rewrites commit IDs in disposable copies but preserves each
file's ancestry under its new prefix. Keep a checked-in mapping from old tips
and release tags to new commit IDs. If preserving original commit IDs matters
more than a clean prefix history, `git subtree` is the simpler alternative,
but file history is usually less pleasant to inspect.

Never rewrite either original remote. Archive them read-only only after the
new repository has been independently cloned and verified.

## Root commands

Expose thin, discoverable wrappers rather than inventing one cross-platform
build system:

```text
make macos-test
make macos-app
make macos-package
make linux-headless
make linux-desktop
make linux-runtime
make linux-clean-runtime
make contracts
```

Each target should delegate to the existing platform Makefile or script.
Platform-specific commands remain directly runnable and authoritative. CI
should use path filters but always run `make contracts` when `contracts/` or
`engine/` changes.

## Releases and tags

- Use `macos-vX.Y.Z` for macOS product releases.
- Use `linux-v0.X.Y-alpha.N` while Linux remains experimental, then
  `linux-vX.Y.Z`.
- Use `engine-vX.Y.Z` only if `libkitty` gains an independently supported ABI
  release cadence.
- Reserve `kitmux-vX.Y.Z` for a future coordinated release whose version and
  contracts genuinely apply to both hosts.

One GitHub release may contain multiple platform artifacts only when they
ship from the same coordinated tag. Otherwise publish separate, clearly named
releases. Never imply that an artifact passed another platform's gate.

## What remains separate

- Swift/AppKit and Linux toolkit host code.
- Xcode/macOS packaging and Linux CMake/native packaging.
- Developer ID/notarization credentials and Linux signing keys.
- VM images, ignored dependency caches, generated build trees, and release
  artifacts.
- Platform-specific lifecycle, paths, shortcuts, desktop integration, and
  browser implementations.
- The landing page deployment.

## Landing page

Keep the landing page in its separate repository. Its static-site deployment,
domain settings, analytics, and publishing cadence are independent of native
application builds and signing. Separation also keeps website dependencies
and deployment credentials out of the GPL-sensitive desktop source tree.

The tradeoff is one extra repository and occasional coordination when release
links or screenshots change. That cost is smaller than coupling website
deployments to a large native monorepo. Automate version/link updates later if
coordination becomes painful; do not move the site during the code monorepo
migration.
