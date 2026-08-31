# TIC-80 vendored reference set

This directory is an offline snapshot of the public official TIC-80 reference
material used to implement and review the GlassVM bundle.

## Sources

| Snapshot | Upstream | Revision |
|---|---|---|
| `corpus/wiki/` | <https://github.com/nesbox/TIC-80.wiki.git> | `3b6a1ff0d33625e933b57e8b802e16e6a8470802` |
| `corpus/upstream-source/` | <https://github.com/nesbox/TIC-80.git> | `4aba09c98f1e5028b82765be1647677b08d35942` |
| `corpus/learn.html` | <https://tic80.com/learn> | retrieved 2026-07-30 |

The wiki snapshot includes every page and image present in the official wiki
repository at the recorded revision. The upstream-source snapshot includes the
project overview, license notices, public embedding headers, and platform build
notes. `corpus/learn.html` preserves the official generated specification, command,
and API help page.

The upstream project and the copied source documentation are MIT licensed; see
`corpus/upstream-source/LICENSE`. Individual wiki pages may contain community
contributions. They are preserved verbatim with provenance rather than treated
as original project documentation.

## Implementation-critical pages

- `corpus/wiki/.tic-File-Format.md`
- `corpus/wiki/RAM.md`
- `corpus/wiki/API.md`
- `corpus/wiki/TIC.md`, `BOOT.md`, `BDR.md`, and `OVR.md`
- `corpus/wiki/btn.md`, `btnp.md`, `peek.md`, `poke.md`, `pix.md`, and `vbank.md`
- `corpus/wiki/Supported-Languages.md`
- `corpus/upstream-source/include/tic80.h`

## Refresh procedure

Clone both upstream repositories, copy the wiki tree without its `.git`
directory, update the selected source documents and public headers, download
`https://tic80.com/learn`, then update the revisions and retrieval date above.
Machine-readable provenance is in `sources.toml`; `SHA256SUMS` covers the
`corpus/` tree exactly.
