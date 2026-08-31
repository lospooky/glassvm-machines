# PICO-8 bundle implementation

`pico8_core` owns `.p8`, `.p8.png`, and `.p8.rom` decoding plus the bounded,
deterministic callback runtime. `pico8_verifier` owns non-executing source and
API compatibility analysis. `pico8_plugin` owns the GlassVM contracts,
sessions, evidence channels, and version-2 state-only session snapshots.

The compatibility runtime executes translated cartridge Lua through a sandboxed
vendored Lua 5.4 engine. Session snapshots are continuation artifacts, not
trace or replay-history containers; machine and emulator identity strings are
unchanged by the snapshot-format break.
