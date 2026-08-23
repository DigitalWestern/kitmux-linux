# macOS window snapshots

Updated 2026-08-23 alongside the macOS multi-window Task 2.

macOS `AppSnapshot` now has an additive optional `windows` array. Each entry
contains a stable window ID, an optional active workspace ID, and sidebar
visibility. The schema remains v1 and the field is absent from legacy files;
the portable fixture corpus therefore stays byte-identical.

Before the Linux port writes or imports its own macOS-compatible state, account
for this optional field explicitly: absent or empty means the legacy
single-window projection, while present entries are hostile input and must be
bounded, deduplicated, and repaired before any live window/runtime objects are
created. Older macOS builds ignore the field and erase it on their next save.

This is a compatibility note, not a request to add Linux multi-window UI.
