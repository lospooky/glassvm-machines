# Hexwell smoke ROM

`smoke.rom` is one complete 256-byte Hexwell catalyst plate. Byte `n` at
well `n` makes the fixture cover every possible catalyst byte exactly once
while retaining the normative row-major 16×16 container shape.

The file is original test data authored for this repository. Reproduce it
offline from this directory with:

```bash
python3 build_fixture.py --output smoke.rom
```

SHA-256 is recorded here after generation:

```text
40aff2e9d2d8922e47afd4648e6967497158785fbd1da870e7110266bf944880  smoke.rom
```
