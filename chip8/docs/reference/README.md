# CHIP-8 reference provenance

This bundle does not vendor third-party game ROMs. Its canonical smoke ROM is
constructed from two documented CHIP-8 opcodes:

- `00 E0`: clear the display
- `12 00`: jump to address `0x200`

The reproducible hexadecimal source is `../../fixtures/source/smoke.hex`;
`smoke.rom` is generated with:

```sh
cd ../../fixtures
python3 build_fixture.py --output smoke.rom
```

Fixture hashes now live with the fixture itself. This reference directory
records captured design material and its independent provenance.

`corpus/history/legacy-verifier-Cargo.lock` is inert archival data. Experiment
020 continues to expose its frozen logical provenance key,
`machines/chip8/chip8_verifier/Cargo.lock`, through an explicit compatibility
binding while hashing these byte-identical archived contents. The archive has
SHA-256 `e4898b24193d27564194c29f568ca40413d455228acb5c955e5f02a8e55ab78f`;
it is never an active Cargo input.
