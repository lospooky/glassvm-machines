# TIC-80 E3 cohort

`cohort.json` pins 30 entries screened for 600 empty-input frames with
frame-completion events and frame hashes. Thirteen passing carts are included
in the fixture tree with their sources and license notices. Three entries reuse
the existing CE1 artifact files; the other included artifacts are under
`paper/e3/`. The other 17 passing carts are
metadata-only: the manifest contains their titles, page/download URLs, and
catalog MD5 and SHA-256 values, but the machine repository does not retain
their bytes.

The 13 included artifacts comprise eight MIT-marked programs from the pinned
pakettic source corpus and five public-site carts with explicit, non-template
license markers in the cart source. Those markers are recorded as the basis
for including the five carts; the general TIC-80 site terms still say that
uploads remain their authors' property and may not be redistributed without
permission. License evidence and the screen records are documented in the
manifest and the GlassVM paper experiment record
`e3-tic80-screen-0.3.3-20261114`.

Rebuild the included `.tic` artifacts offline:

```sh
python3 tic80/fixtures/build_e3_cohort.py
```

This rebuild does not fetch or materialize the 17 metadata-only carts.
