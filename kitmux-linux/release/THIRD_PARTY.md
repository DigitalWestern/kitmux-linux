# Kitmux Linux engine third-party inventory

The release builder derives the shipped native-library closure from the ELF
objects instead of copying the entire Kitty development bundle. Every runtime
component below has a tracked verbatim notice under `release/licenses/` and a
corresponding package in the generated SPDX 2.3 JSON SBOM at
`share/kitmux-engine.spdx.json`.

| Component | Version | License |
| --- | --- | --- |
| kitty and libkitty | `c1d507dbe8cd12830d8b97b0d350d9dc2e4d383f` | GPL-3.0-only |
| Symbols Nerd Font Mono | 3.4.0 | MIT |
| CPython | 3.14.6 | PSF-2.0 |
| zlib | 1.3.2 | Zlib |
| bzip2 | 1.0.8 | bzip2-1.0.6 |
| OpenSSL | 3.5.7 | Apache-2.0 |
| Expat | 2.6.2 | MIT |
| libxkbcommon | 1.7.0 | MIT |
| SQLite | 3.53.2 | public domain |
| libffi | 3.4.6 | MIT |
| ncurses | 6.5 | X11 |
| GNU Readline | 8.2 | GPL-3.0-or-later |
| XZ Utils liblzma | 5.8.3 | 0BSD |
| xxHash | 0.8.2 | BSD-2-Clause |
| libpng | 1.6.57 | libpng-2.0 |
| Little CMS | 2.17 | MIT |
| Brotli | 1.1.0 | MIT |
| Pixman | 0.44.2 | MIT |
| FreeType | 2.13.2 | FTL OR GPL-2.0-only |
| Fontconfig | 2.17.1 | MIT |
| Cairo | 1.18.2 | LGPL-2.1-only OR MPL-1.1 |
| HarfBuzz | 12.3.0 | MIT |
| Wayland | 1.24.0 | MIT |

`release/runtime-components.json` is the machine-readable source of truth for
versions, licenses, source URLs, source archive hashes, runtime file mappings,
and notice names. `refresh-license-notices.py` downloads those exact archives,
checks every SHA-256, and reconstructs the tracked notices. The release audit
fails if a shipped file has zero or multiple owners, if a component has no
payload, if a notice is missing, or if an SBOM file checksum is stale.
