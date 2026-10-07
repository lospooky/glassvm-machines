# TIC-80 bundle implementation

`tic80_core` owns `.tic` chunk decoding, the 96 KiB memory model, the bounded
Lua compatibility VM, the Rust implementation of TIC-80 host APIs and drawing,
gamepad input, framebuffer output, and native runtime state.
`tic80_verifier` owns non-executing structure, callback, language, and Lua
syntax checks. `tic80_bundle` owns GlassVM contracts, session policy,
normalized evidence, and version-2 state-only session snapshots.

Core is also the single authority for `MACHINE_ID`, `SEMANTICS`, and native
runtime configuration. `cycles_per_frame` is exactly one. The only declared
machine parameter is `runtime`; when absent it defaults to `lua`, while an
explicit non-string, a value other than `lua`, or any unknown key is rejected.
Scheduled inputs and the snapshot cursor are checked against the frozen frame
budget before any restoration mutates runtime state. The state-only snapshot
format intentionally breaks from the previous history-backed format: old or
incompatible snapshots are rejected explicitly and the run must be restarted.

All machine-side host behavior is Rust. Lua source remains cartridge data, and
the vendored Lua VM remains the compatibility engine because executing Lua
cartridges is part of this variant's declared semantics. Replacing that VM is a
language-compatibility project, not an envelope reorganization; the assessment
is preserved in the dated history record.
