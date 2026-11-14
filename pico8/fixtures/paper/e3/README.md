# PICO-8 E3 cohort

This directory contains the E3 manifest, attribution, and the adapted Fire
Intro manifest. The adapted Fire Intro artifact is reused from
`../../fireintro.p8`. The 29 reconstructed Fraser carts use canonical
files from `../ce1/` and `../../source/e3/`; they are not duplicated here. Attribution,
source, license metadata, screen results, and SHA-256 digests are in
`cohort.json`. The set has mixed provenance: 29 reconstructed Fraser dataset
carts with per-cart CC BY-NC-SA 4.0 markers and one adapted MIT-licensed Fire
Intro cart.

See [`ATTRIBUTION.md`](ATTRIBUTION.md) for per-cart credits, source links,
license links, digests, and the Fire Intro adaptation note. The reconstructed
carts are distributed under their recorded per-cart CC BY-NC-SA 4.0 terms:
attribution, noncommercial use, and ShareAlike apply. These third-party assets
are separate from the repository's BSD-3-Clause code license. Fire Intro's
upstream MIT license text is retained under `../../source/public/`.

Verify the pinned cohort, rebuilding only the adapted Fire Intro artifact, with:

```sh
python3 pico8/fixtures/build_e3_cohort.py
```

The 600-frame screening evidence and exact screening environment are linked
from the cohort manifest to the GlassVM paper-publication records. The screen
is an eligibility check, not a performance measurement or representative
sample of all PICO-8 programs. Dataset license markers are attributed to the
pinned Fraser dataset; this record is not a separate per-author rights review.
