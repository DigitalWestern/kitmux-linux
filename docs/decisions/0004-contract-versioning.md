# ADR 0004: Version observable contracts, not host internals

Status: accepted

Snapshots and control framing retain version 1 until a breaking wire-format
change is necessary. Additive optional fields do not bump the version.
Command identifiers and error codes are stable strings.

Both hosts must consume the same valid and invalid fixtures. Bounds and
malformed-input behavior are part of the contract. Platform paths, shortcuts,
browser data, and executable discovery are explicitly outside it.

