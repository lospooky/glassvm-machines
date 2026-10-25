# Canonical smoke ROM

`smoke.rom` is a four-byte CHIP-8 program constructed for this repository. It
clears the display (`00 E0`) and loops at `0x200` (`12 00`). It contains no
third-party game code. `source/smoke.hex` is its reproducible hexadecimal
source. Rebuild it offline with:

```sh
python3 build_fixture.py --output smoke.rom
```
