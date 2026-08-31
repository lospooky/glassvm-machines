# TIC-80 bundle testing

Core tests parse and execute the real upstream Gecko `.tic` cartridge, compare
deterministic native runs, and prove exact replay-backed snapshot continuation.
Verifier tests analyze the real cartridge and reject syntactically invalid Lua
without executing it. Adversarial cases cover quoted and long-bracket strings,
short and equals-delimited long comments, local shadowing, table/nested
near-matches, malformed declarations, and accepted top-level declarations and
assignments. Plugin tests cover contract/body resolution, strict native config
and directly deserialized stimuli, real-cart native-event normalization and
causality, one-shot lifecycle closure, bounded runtime/sink failures, atomic
snapshot rejection, scheduled-input replay, reset, and exact session resume.
Snapshot tests additionally cover executable fresh restores, explicit v3
lifecycle round-trips, terminal/failed poisoning, rewind/divergence rejection,
and atomic rejection of legacy, malformed, or contradictory envelopes.

The smoke artifact rebuilds offline from its byte-identical upstream source and
both fixture and reference checksum manifests cover their corpora exactly.
