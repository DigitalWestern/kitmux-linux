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
documented in [`PORTABLE_FIXTURES.md`](PORTABLE_FIXTURES.md). Slice 0.4 freezes
the macOS producer/consumer side; Phase 3 adds the Linux model consumer
without changing fixture expectations.

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
it. References of the form `path::testName` name a real test function in the
tagged baseline; references naming only a file mean the behavior is covered
there but has not yet been pinned to specific cases.

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
source and test reference exists in the baseline checkout. Run it whenever the
inventory or the macOS baseline changes — a traceability claim nobody checks
decays into decoration.
