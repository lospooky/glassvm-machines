# Wyrd-16 testing

Core tests cover artifact bounds, deterministic transitions, published drawing
semantics, and native snapshot continuation. Verifier tests cover the fixture's
static profile and tolerant malformed-artifact diagnostics. Plugin tests cover
the bundle contract, default body, full ROM execution, native and observable
adapters, input replay, trace collection, and session snapshot/reset behavior.

From the repository root:

```bash
cargo test -p wyrd16_core -p wyrd16_verifier -p wyrd16_plugin
cargo clippy -p wyrd16_core -p wyrd16_verifier -p wyrd16_plugin \
  --all-targets --no-deps -- -D warnings
(cd machines/wyrd16/fixtures && python3 build_fixture.py --output smoke.rom)
(cd machines/wyrd16/fixtures && sha256sum -c SHA256SUMS)
(cd machines/wyrd16/docs/reference/corpus && sha256sum -c ../SHA256SUMS)
```
