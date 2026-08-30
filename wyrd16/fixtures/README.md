# Wyrd-16 fixtures

`smoke.rom` is the ten-byte drawing artifact from the machine README. It
selects `(10,20)`, plots color 3, and halts. Runes are aligned big-endian
16-bit words.

Rebuild it offline from this directory:

```bash
python3 build_fixture.py --output smoke.rom
sha256sum -c SHA256SUMS
```

`provenance.toml` records the artifact kind, authorship, license reference,
digest, source description, and exact rebuild command.
