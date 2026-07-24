# ADR 0003: Linux repository and reference-source boundary

Status: accepted

`operating-system/linux` is an independent Git repository. The tagged macOS
checkout is the current authoritative libkitty and behavior source. Linux
builds materialize only the locked files from
`macos-linux-port-baseline-2026-07-23` and verify every recorded hash in
`source-lock.json`.

Linux-specific build and GL-loader work lives here during the spikes. Before
beta, decide whether libkitty moves to `home-kitmux/shared`, becomes a separate
versioned repository, or remains a source package consumed by both hosts.
There must never be two manually maintained libkitty implementations.
