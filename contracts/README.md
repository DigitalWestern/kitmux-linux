# Cross-platform contracts

The files here describe observable behavior shared by the tagged macOS
reference and the Linux host. They are not permission to copy AppKit
implementation details.

Contract rules:

- Every fixture declares a version and byte/collection bounds.
- Valid fixtures must round-trip in macOS and Linux tests.
- Invalid fixtures must name reject, repair, or ignore behavior.
- Loading a snapshot, SSH profile, or control request must never execute a
  saved command or initiate a connection.
- Unknown newer contract versions are preserved or set aside, never silently
  rewritten as the current version.

Nonportable values stay out of shared fixtures: absolute user paths, installed
font names, AppKit shortcut encodings, browser cookies/data, shell commands,
agent sockets, keychain references, and platform package paths.

The authoritative JSON corpus is under `fixtures/v1/`. Its envelopes,
malformed-input rules, temporary macOS resource mirror, and exact gates are
documented in [`PORTABLE_FIXTURES.md`](PORTABLE_FIXTURES.md). Slice 0.4 froze
the corpus; Phase 3 now consumes every contract family on Linux and proves
macOS consumption of temporary Linux-produced values without changing fixture
expectations.

## The feature inventory

`feature-inventory.json` is authoritative for classification and observable
Linux acceptance statements.

Schema 2 (2026-07-25) decomposed the original 16 subsystem rows into 64
per-behavior rows. At subsystem granularity, Phase 9's parity gate — "resolve
every parity row as shipped, intentionally different, or deferred" — was
unfalsifiable: a row like `navigation.hierarchy` covered an entire subsystem
against a ~32,000-line macOS application, so it could be marked shipped while
half the subsystem was missing.

Each row now states one observable behavior and names the macOS test that pins
it. A reference is `path` or `path::anchor`. An anchor names a Swift, Python,
or C function definition; the two C gates are one `main()` with no per-case
functions, so their anchors are the `// vN.M feature` comments that label the
block asserting the behavior. A bare `path` means the whole file is the case,
as in `test_engine.c`, or that the file has no finer anchor to name.

Four rows carry an empty `macos_tests`. Anchoring them in July 2026 found that
the file each one named asserts nothing about its behavior at the tag, so the
claim was removed rather than made precise; `notes.macos_test_gaps` records
which rows and why. Parity for those must be argued from macOS sources and the
Linux gate, and saying so is the point — a row that names a test which does not
test it is worse than a row that admits it has none.

The 17 schema 1 IDs are retained as `areas` so earlier references resolve.

### Rules

- One row, one observable behavior. If a row's acceptance statement needs the
  word "and" more than twice, split it.
- `linux_status` defaults to `unproven`. That is a normal state, not a defect.
- A `linux_status` claim must have a matching entry in `PORT_STATUS.md`. A
  status here without one is a bug in the ledger, not in the inventory.
- Rows classified `intentionally-omitted` are not parity obligations and must
  not be counted against parity.

### Checking it

```sh
contracts/validate-inventory.py
```

Verifies structure, unique IDs, resolvable dependencies, and that every macOS
source and test reference resolves — at the locked tag, read through
`git show`, not in the checkout's working tree. The distinction matters: the
macOS repository is allowed to move ahead of the baseline, and a reference that
resolves only against newer work is exactly the silent claim this is here to
catch. A reference that is newer on purpose has to be declared in the script's
`POST_BASELINE_ALLOWED` with its reason, and every run prints it.

Without a macOS checkout the reference pass is skipped, which is why
`test-headless.sh` can run this and `test-phase3.sh` passes the path. Run it
whenever the inventory or the macOS baseline changes — a traceability claim
nobody checks decays into decoration.
