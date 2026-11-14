# PICO-8 smoke cartridge

`smoke.rom` is a native text `.p8` cartridge retained under the repository's
canonical opaque fixture name. It is copied byte-for-byte from
`source/smoke.p8`, which comes from the MIT-licensed `pico8_decompress` 0.1.0
test corpus by Shane Celis. Rebuild it offline with:

```sh
python3 build_fixture.py --output smoke.rom
```

Upstream: <https://github.com/shanecelis/pico8_decompress>

The 910-byte artifact declares cartridge version 41, includes Lua `_init` and
`_draw` callbacks, clears the display, and contains a small graphics section.
Its SHA-256 is:

```text
655dfc34d070882ec84924c0c4c791bbe1d249bc3f4b5885f1f6fb293cbaa739
```

The equivalent encoded PNG and raw ROM forms live in `cases/`. No proprietary
commercial cartridge data is included.

## Paper no-input workload

`paper/fireintro.p8` is built from the MIT-licensed PICO-8 fire effect from
the pinned `thxrsxm/fire-intro-effect` repository recorded in
`provenance.toml`. The source and its MIT license are checked in under
`source/public/`; `build_paper_workload.py` reproduces the paper artifact
without network access.
Its `_update60()` callback evolves a fire grid and `_draw()` renders it. The
paper run uses an empty input schedule and exercises deterministic changing
frames in the documented PICO-8 Lua/API subset. The builder makes one explicit
presentation-only adjustment: it changes the title's P8SCII escapes to ASCII
because the current Lua parser rejects those string escapes. The fire update
and framebuffer logic are unchanged. This is an adapted compatibility
specimen, not an unmodified-cart result and not a claim of complete PICO-8
semantics.

## CE1 no-input cohort

`paper/ce1/` contains the three canonical reconstructed PICO-8 artifacts used
for CE1. Their attribution, per-cart CC BY-NC-SA 4.0 metadata, and 600-frame
eligibility results are recorded in `paper/ce1/cohort.toml`. The initial
30-candidate screen, including its one compatibility failure, is recorded in
the paper experiment records. Verify the pinned artifacts with:

```sh
python3 verify_ce1_cohort.py
```

These are reconstructed dataset artifacts, not byte-identical claims about
the original BBS uploads. No duplicate source copies are kept: the CE1 paths
are canonical and are reused by E3 where the cohorts overlap. The license
applies per cartridge and does not change the repository code license.

## E3 no-input cohort

`paper/e3/` contains the E3 cohort manifest and attribution. Its 29 Fraser
reconstructions use the canonical artifact paths in `paper/ce1/` and
`source/e3/`; the adapted MIT-licensed Fire Intro artifact reuses
`paper/fireintro.p8`. The manifest preserves each artifact's provenance, license
metadata, digest, and 600-frame screen result. The Fraser dataset reports
per-cart CC BY-NC-SA 4.0 markers; those conditions are recorded separately
from the repository code license. See the cohort README for the rebuild and
verification command.
