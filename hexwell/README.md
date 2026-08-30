# Hexwell machine bundle

Hexwell is a deterministic fantasy spatial ISA whose immutable 16×16 catalyst
plate drives synchronous reaction fronts across a toroidal hex lattice.

`core/` owns native catalyst decoding, reactor state, sweeps, projection, and
snapshots. `verifier/` owns non-executing plate analysis and diagnostics.
`plugin/` owns only GlassVM contracts, tide-body translation, session and
replay orchestration, adapters, traces, and request-bound snapshots.

The complete 256-byte executable specimen and its offline builder live in
`fixtures/`.
