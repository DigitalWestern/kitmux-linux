# ADR 0005: Fast development Python, isolated release Python

Status: accepted

Development builds may embed the distribution Python named in
`support-matrix.yml`. Release artifacts must bundle an audited Python runtime,
Kitty modules, configuration, fonts metadata, and native libraries. ELF
lookups use `$ORIGIN`-relative runpaths and must not fall back to host Python.

The first portable artifact is a relocatable `tar.zst`. Native `deb` and `rpm`
packages wrap that proven layout. Automatic updating is out of process for the
terminal alpha; distro/package-manager ownership is the default update
boundary.

For a GTK process, isolation is narrower: bundle or isolate the pinned
`libpython`, but use the distribution's GTK, Cairo, HarfBuzz, GLib, xkbcommon,
and graphics stack. Kitty's full private dependency directory must not appear
in the GUI's global loader path. If a future self-contained desktop package
cannot maintain that boundary safely in one process, isolate the engine in a
separate process rather than using broad `LD_LIBRARY_PATH`.
