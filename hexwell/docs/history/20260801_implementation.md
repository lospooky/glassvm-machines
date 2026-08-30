# Hexwell legacy implementation record

Before the canonical three-crate migration, the implementation was split as follows:

- `src/isa.rs` decodes catalyst families and lattice addressing.
- `src/machine.rs` owns reactor state, synchronous sweeps, and projection.
- `src/plugin.rs` implements the bundle, sessions, adapters, analyzer,
  verifier, and snapshot validation.
- `src/tests.rs` contains semantic, conservation, and adversarial vectors.
- `tests/bundle.rs` is the public, on-disk artifact regression.

All 256 catalyst bytes decode, while the loader requires exactly 256 bytes.
The smoke plate contains byte values `0..=255` in well order, so one small
artifact exercises every decoder input. Its reproducible recipe and digest
now live in the bundle-level `fixtures/` directory.
