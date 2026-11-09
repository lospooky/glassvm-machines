# Public CHIP-8 workload sources

These exact upstream files are inputs to the CHIP-8 CE1 cohort. The cohort
manifest at `../../paper/cohort.toml` records their SHA-256 digests, profile
screen, source revisions, and license evidence.

- `2-ibm-logo.ch8`: Timendus CHIP-8 test suite, revision
  `742e9eac9f5d44466c1c9d83fcaa8158c6683cb1`, GPL-3.0.
- `horseWorldOnline.ch8`: John Earnest's CHIP-8 archive, revision
  `a8640ed9a7d35f73a12e24fcf560e38b5d000001`, CC0-1.0.
- `octojam2title.ch8`: John Earnest's CHIP-8 archive, same pinned revision,
  CC0-1.0.

The ROMs are distinct third-party specimens. Their upstream licenses apply to
the ROM files; they are not relicensed under this repository's BSD-3-Clause
code license. The IBM Logo Test also has a secondary archive manifestation
with unknown license metadata; this cohort pins the direct Timendus source.
