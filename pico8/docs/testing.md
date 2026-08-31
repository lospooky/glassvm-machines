# PICO-8 bundle testing

Core tests cover all three native cartridge formats, deterministic callback
execution, controller input, graphics, sandbox budgets, and native snapshots.
Verifier tests cover static analysis and malformed artifacts. Plugin tests cover
the body, execution, adapters, trace collection, replay, and exact session
continuation against `fixtures/smoke.rom`.

The smoke artifact is rebuilt offline from `fixtures/source/smoke.p8` and
compared byte-for-byte. Fixture and reference checksum manifests cover their
respective corpora exactly.

```sh
cargo test --locked -p pico8_core -p pico8_verifier -p pico8_plugin --all-targets
cargo clippy --locked -p pico8_core -p pico8_verifier -p pico8_plugin \
  --all-targets --no-deps -- -D warnings
```
