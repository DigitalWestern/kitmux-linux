# Kitmux Linux engine third-party inventory

This is the initial source and license inventory for the engine proof bundle.
It is intentionally marked incomplete: do not distribute a release until every
copied shared library has a recorded source, version, license, and required
notice.

| Component | Source used by the proof | License status |
| --- | --- | --- |
| libkitty | Locked Kitmux macOS baseline | GPL-3.0, inherited from Kitty |
| Kitty | Pinned `c1d507dbe8cd12830d8b97b0d350d9dc2e4d383f` | GPL-3.0 |
| CPython 3.14 | Kitty Linux dependency bundle | PSF license; notice pending |
| Symbols Nerd Font Mono | Pinned Kitty source | License notice pending |
| Native shared libraries | Kitty Linux dependency bundle | Per-library inventory pending |

The build creates `share/SHA256SUMS` so the exact runtime inputs can be
matched to the completed inventory.
