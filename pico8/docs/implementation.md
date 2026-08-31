# PICO-8 bundle implementation

`pico8_core` owns `.p8`, `.p8.png`, and `.p8.rom` decoding plus the bounded,
deterministic callback runtime. `pico8_verifier` owns non-executing source and
API compatibility analysis. `pico8_plugin` owns only GlassVM contracts, body
translation, sessions, normalized evidence, replay, and session snapshots.

The compatibility runtime executes translated cartridge Lua through a sandboxed
vendored Lua 5.4 engine. Machine and emulator identity strings are unchanged by
the envelope migration.
