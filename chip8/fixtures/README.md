# Canonical smoke ROM

`smoke.rom` is a four-byte CHIP-8 program constructed for this repository. It
clears the display (`00 E0`) and loops at `0x200` (`12 00`). It contains no
third-party game code. `source/smoke.hex` is its reproducible hexadecimal
source. Rebuild it offline with:

```sh
python3 build_fixture.py --output smoke.rom
```

The paper branch contains the three public, zero-input CHIP-8 specimens in
`paper/`: `2-ibm-logo.ch8`, `horseWorldOnline.ch8`, and
`octojam2title.ch8`. Their filenames preserve the upstream names. The first
two exact inputs are kept under `source/public/`; the Octojam 2 source was
already present there. License and revision pins are in `paper/cohort.toml`.
The files are separate third-party fixtures, not covered by the repository's
BSD-3-Clause license. Rebuild any paper copy offline with:

```sh
python3 build_paper_workload.py --source source/public/2-ibm-logo.ch8 --output paper/2-ibm-logo.ch8
python3 build_paper_workload.py --source source/public/horseWorldOnline.ch8 --output paper/horseWorldOnline.ch8
python3 build_paper_workload.py --output paper/octojam2title.ch8
```

The cohort is the CHIP-8 subset for CE1. Each passed the empty-input
600-frame eligibility screen; the exact hashes and source/license records are
listed in `paper/cohort.toml`. The four-byte `smoke.rom` remains the small
conformance fixture.

The E3 corpus is a separate 30-ROM cohort in `paper/e3/cohort.json`. It
contains the 30 CC0-1.0 candidates among the 37 programs that passed the
600-frame empty-input screen; selection was by the pinned archive license
marker, not by behavioral ranking. The two artifacts already present for
CE1 are reused by reference, and each other ROM is stored once under
`paper/e3/`. The manifest records upstream URLs, hashes, screen outcomes, and
license evidence. Verify the pinned files offline with:

```sh
python3 chip8/fixtures/verify_e3_cohort.py
```
