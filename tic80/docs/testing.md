# TIC-80 bundle testing

Core tests parse and execute the real upstream Gecko `.tic` cartridge, compare
deterministic native runs, and prove state-only snapshot continuation.
Verifier tests analyze the real cartridge and reject syntactically invalid Lua
without executing it. Adversarial cases cover quoted and long-bracket strings,
short and equals-delimited long comments, local shadowing, table/nested
near-matches, malformed declarations, and accepted top-level declarations and
assignments. Plugin tests cover contract/body resolution, strict native config
and directly deserialized stimuli, real-cart native-event normalization and
causality, one-shot lifecycle closure, bounded runtime/sink failures, atomic
snapshot rejection, scheduled-input replay, reset, and exact session resume.
Snapshot tests additionally cover the version-2 state schema, explicit
incompatible-version rejection, absence of trace and input-history vectors,
and continuation in a fresh session without persisted observational history.

The smoke artifact rebuilds offline from its byte-identical upstream source and
both fixture and reference checksum manifests cover their corpora exactly.
