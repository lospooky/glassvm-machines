# Canonical smoke ROM

`smoke.rom` is a four-byte CHIP-8 program constructed for this repository. It
clears the display (`00 E0`) and loops at `0x200` (`12 00`). It contains no
third-party game code. `source/smoke.hex` is its reproducible hexadecimal
source. Rebuild it offline with:

```sh
python3 build_fixture.py --output smoke.rom
```

The public zero-keypad paper workload is the CC0-1.0 Octojam 2 animated title
cartridge at `paper/octojam2title.ch8`, retaining its native `.ch8` extension
and upstream filename. Its exact upstream input is preserved at
`source/public/octojam2title.ch8`. Rebuild the paper copy offline with:

```sh
python3 build_paper_workload.py --output paper/octojam2title.ch8
```

The cartridge exercises the CHIP-8-profile display/animation path without
scripted input; the four-byte `smoke.rom` remains the small conformance fixture.
