# ADR 0003: Linux repository and reference-source boundary

Status: accepted for the current spike; long-term monorepo migration proposed

`operating-system/linux` is an independent Git repository. The tagged macOS
checkout is the current authoritative libkitty and behavior source. Linux
builds materialize only the locked files from
`macos-linux-port-baseline-2026-08-02-v0.21` and verify every recorded hash in
`source-lock.json`.

Linux-specific build and GL-loader work lives here during the spikes. The
recommended long-term destination is one history-preserving monorepo with
platform-specific trees, root contracts, and one authoritative
`engine/libkitty`; see [`../MONOREPO_MIGRATION.md`](../MONOREPO_MIGRATION.md).

That migration is not executed by this decision record. Until it is, the
hash-locked tagged-source boundary remains authoritative. There must never be
two manually maintained libkitty implementations.
