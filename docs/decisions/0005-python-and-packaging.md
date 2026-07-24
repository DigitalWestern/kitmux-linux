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

