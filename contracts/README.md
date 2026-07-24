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

`feature-inventory.json` is authoritative for classification and observable
Linux acceptance statements. Concrete JSON fixtures will be added under
`fixtures/v1/` and become authoritative only when both hosts consume them.
