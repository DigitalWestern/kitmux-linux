# Portable contract fixtures

`fixtures/v1/` is the authoritative, display-free contract corpus shared by
the tagged macOS reference and the future Linux model. The fixture-envelope
version is independent of the state, settings, control, and SSH document
versions carried inside individual cases.

Every contract file declares:

- `fixtureVersion`;
- explicit byte or collection bounds;
- named cases with an `accept`, `repair`, `reject`, `ignore`, or `preserve`
  disposition;
- only deterministic values that another host can reproduce.

The corpus covers state snapshots and stable IDs, settings defaults and
validation, split-tree collapse behavior and close order, stable command
identifiers, control framing and limits, and portable SSH profile/review data.

## Authority and mirrors

The Linux repository owns the canonical files. The macOS package carries a
byte-identical test-resource mirror at:

```text
macos/KitmuxApp/Tests/KitmuxCoreTests/Fixtures/Portable/v1/
```

The mirror is temporary repository plumbing, not a second source of truth.
`validate-fixtures.py --mirror <directory>` fails if any JSON byte differs or
either side gains an undeclared file. A future monorepo should make both test
targets consume one physical directory and remove the mirror.

## Malformed-input behavior

- State navigation indices and counters are repaired when the intended value
  is unambiguous. Empty containers and duplicate stable IDs are rejected.
- Invalid or unknown settings keys are ignored independently and resolve to
  that key's default. They do not invalidate other settings.
- Duplicate pane IDs make a split tree invalid. Removing panes follows the
  declared order and collapses an empty split branch into its sibling.
- Unknown command identifiers are rejected. Display shortcuts are not command
  identifiers and are deliberately absent.
- Control frames reject malformed JSON, unsupported versions, invalid
  envelope fields, and byte-limit violations with the declared error code.
- SSH documents reject duplicate names and control characters. Review cases
  parse recorded `ssh -G` text only; they never launch `ssh` or connect.

Unknown newer state, settings, control, or SSH versions are preserved or set
aside by their owning store. A fixture is never silently rewritten to an older
version.

## Values that are not portable

| Value | Why it stays host-owned |
| --- | --- |
| Absolute user/config/cache paths | Linux uses XDG paths; macOS uses Application Support and other platform locations. |
| Installed font names and font discovery | Availability and fallback differ by distribution and desktop. |
| Display shortcut strings and raw key codes | AppKit, GTK, and desktop environments use different key vocabularies. Stable command IDs are portable. |
| Browser cookies, caches, website data, and history | These belong to the selected WebKit host and may contain private data. |
| Shell, resume, remote, proxy, or agent commands | Commands are inert import data at most. They require validation and explicit user approval before any later execution path. |
| Agent/control socket paths | Address formats, path limits, and runtime directories differ by host. |
| Keychain or secret-store references | Credential stores and access controls are platform-specific. |
| Package-manager and executable paths | Package formats and installed locations are support-matrix concerns. |

Rejected security fixtures may contain a deliberately malformed command string
to prove it is rejected. Accepted portable fixtures never persist a command,
socket, secret reference, or absolute platform path.

## Gates

From the Linux repository:

```sh
contracts/validate-fixtures.py
contracts/validate-fixtures.py --mirror \
  ../macos/kitmux/macos/KitmuxApp/Tests/KitmuxCoreTests/Fixtures/Portable/v1
```

From `macos/KitmuxApp`:

```sh
swift test --filter PortableContractFixtureTests
```

The macOS suite has both producer and consumer assertions. Producer assertions
compare values built through `KitmuxCore` with the resource files; consumer
assertions decode the files and apply the production validators. No fixture
test creates a process, opens a socket, resolves a host, or writes a command
to a terminal.
