# TIC-80 bundle implementation

`tic80_core` owns `.tic` chunk decoding, the 96 KiB memory model, the bounded
Lua compatibility VM, the Rust implementation of TIC-80 host APIs and drawing,
gamepad input, framebuffer output, and native replay-backed snapshots.
`tic80_verifier` owns non-executing structure, callback, language, and Lua
syntax checks. `tic80_plugin` owns only GlassVM contracts, body translation,
session policy, normalized evidence, and session-bound snapshots.

Core is also the single authority for `MACHINE_ID`, `SEMANTICS`, and native
runtime configuration. `cycles_per_frame` is exactly one. The only declared
machine parameter is `runtime`; when absent it defaults to `lua`, while an
explicit non-string, a value other than `lua`, or any unknown key is rejected.
Scheduled inputs and snapshot histories are checked against the frozen frame
budget and replay plan before any restoration mutates runtime state.
Replay-envelope v3 also preserves the explicit session lifecycle. It rejects
legacy lifecycle-lossy snapshots, fresh snapshots with non-reset state, and
stale or divergent input histories atomically.

All machine-side host behavior is Rust. Lua source remains cartridge data, and
the vendored Lua VM remains the compatibility engine because executing Lua
cartridges is part of this variant's declared semantics. Replacing that VM is a
language-compatibility project, not an envelope reorganization; the assessment
is preserved in the dated history record.
