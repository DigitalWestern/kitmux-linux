# ADR 0007: What the GTK spike carries forward, and what it does not

Status: accepted

## Context

ADR 0002 called the GTK spike disposable and free of production UI. ADR 0001
named Rust as the model and lifecycle language. Both were true when written.

They stopped being true during Slices 2.2A and 2.2B. `NEXT_STEPS.md` already
instructs later sub-slices to *inherit* `src/gtk_key_translation.{c,h}`, the
PTY recorder fixture, the XTEST injector, and the host's stdout reporting
contract. That is the correct engineering call — those artifacts encode real,
expensively-acquired knowledge:

- functional-key numbering and Kitty's GLFW-fork modifier bits;
- base-layout key identity resolved through `gdk_display_translate_key`, with
  the shifted codepoint carried alongside;
- Kitty's text-suppression rules;
- the two input-method commit routes, and why each exists;
- withholding a release whose press never reached the terminal as a key event;
- press-versus-auto-repeat tracking keyed on hardware keycode.

Roughly 1,400 lines of C now sit in a tree that two decision records describe
as throwaway. Nobody had decided whether the eventual Rust host calls this code
or replaces it. Slices adding selection, clipboard, mouse, and search would
have doubled that ambiguity before anyone resolved it.

## Decision

Split the spike explicitly into two categories, and label every file.

### Durable — survives the toolkit decision, in C, called over FFI

- `src/gtk_key_translation.{c,h}` — display-free GDK-to-`kitty_key_event`
  translation and the held-key tracker.
- `tests/gtk_key_matrix.c` — the fixed-byte expectation matrix.
- `tests/pty_input_recorder.c` — the recording child fixture.
- `tests/x11_key_injector.c` — XTEST synthesis.

These stay C permanently. The future Rust host binds them through the same
`extern "C"` boundary it already uses for `libkitty`; the Rust/C layout check
in `rust/header-smoke` extends to cover `kitmux_gdk_key_input`,
`kitmux_key_translation`, and `kitmux_key_tracker`. Rewriting this translation
in Rust is explicitly *not* planned: it would re-derive Kitty's key semantics
from scratch with no test oracle other than the C version it replaced.

The durable set must keep its current properties, which are what make it
portable:

- no `GdkDisplay` dependency and no GTK widget state;
- no I/O, no allocation, no global state beyond the caller-owned tracker;
- expectations derived from Kitty's pinned `key_encoding.c`, never captured
  from this host's own output.

Any change that breaks one of those properties needs a new ADR.

### Disposable — dies with the toolkit decision

- `src/gtk_terminal_host.c` — window construction, widget wiring, the GLib PTY
  source, the diagnostic overlay, the preedit overlay label, and all
  `KITMUX_GTK_*` harness environment variables.

This file is a probe. It is allowed to be a single flat translation unit with
harness reporting on stdout, and it must not accumulate product behavior. It
will be replaced wholesale by the Phase 4 application shell.

## Rule for new spike work

Before adding code to the spike, place it in one category. If it is durable, it
belongs in its own display-free translation unit with a headless test. If it is
disposable, it goes in `gtk_terminal_host.c` and stays as small as it can be.

Code that answers "can this toolkit do X at all?" is spike work. Code that
implements how Kitmux does X is product work and belongs in Phase 4, after the
toolkit decision, in the chosen host language. Slices 2.2C through 2.2E were
reclassified on this basis; see ADR 0002's 2026-07-25 addendum and the Phase 2
section of the plan.

## Consequences

- The GTK-versus-Qt decision no longer risks the keyboard work. A Qt probe
  would replace `gtk_terminal_host.c` and re-target the translation unit's
  input struct, not restart the key semantics.
- `rust/header-smoke` grows a second responsibility and must be run whenever
  the durable headers change.
- ADR 0001's "GUI host language remains open" stands. This ADR narrows it: the
  host language is open, the key-translation language is not.
