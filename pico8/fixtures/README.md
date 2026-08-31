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
