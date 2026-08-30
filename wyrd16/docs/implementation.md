# Wyrd-16 implementation

The implementation follows the canonical three-crate envelope. Native rune
decoding and every machine transition live in `core`; static profiling and
diagnostics live in `verifier`; GlassVM descriptor, contract, body, session,
trace, snapshot-envelope, replay, and adapter concerns are separate files in
`plugin`.

Every aligned 16-bit word decodes. The loader accepts non-empty even lengths
up to the 4 KiB arena size and zero-fills the remaining memory. The fixture is
the published ten-byte pixel program; `fixtures/provenance.toml` binds its
origin, digest, and deterministic builder.

The implementation deliberately keeps execution free of ambient services:
the only runtime input is the frozen request, declared key stimuli, and seeded
native entropy. Machine identity remains `wyrd16-semantics.v1`; the GlassVM
session snapshot format remains version 2.
