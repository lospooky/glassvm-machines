# TIC-80 bundle architecture

The TIC-80 bundle has three machine-owned crates with one-way dependencies:

```text
tic80_plugin -> tic80_verifier -> tic80_core
            `--------------------> tic80_core
```

Only `tic80_plugin` depends on GlassVM. `tic80_core` contains the native Rust
cartridge parser, fantasy-console memory, host APIs, drawing, bounded Lua
compatibility runtime, and native snapshots. `tic80_verifier` contains
non-executing analysis and acceptance policy. The plugin contains contract,
body, lifecycle, evidence, and adapter integration only.

## Execution path

1. `parse_cart` bounds and decodes the native chunk stream, assembling
   multi-bank or compressed Lua source and mapping bank-zero assets into the
   documented 96 KiB memory image.
2. The native analyzer reports cartridge language, source size, chunk layout,
   callback presence, and artifact identity. The native verifier rejects
   malformed, non-Lua, callback-free, or syntactically invalid carts without
   executing cartridge code. Callback discovery is deliberately bounded: a
   small Lua-aware lexer removes short/long comments and quoted/long-bracket
   strings, tracks top-level local shadowing and table/function scope, and
   recognizes conventional global `TIC` declarations or assignments. An
   independent `mlua` compile-only pass gates that evidence. This is not a
   claim that the verifier contains a complete second Lua parser.
3. The core builds a vendored PUC-Rio Lua 5.4 compatibility VM through `mlua`,
   applies a 16 MiB heap limit and one-million-instruction hook, installs the
   allowlisted TIC-80 API, executes top-level code, and calls `BOOT()` when
   present.
4. Each native 60 Hz step applies four-gamepad input, calls `TIC()`, updates the
   packed framebuffer and machine state, and returns typed input, trace, and
   completed-frame events. The plugin feeds those native events through
   `Tic80NativeEventAdapter`; normalized frame effects retain their sampled-input
   causal link.
5. Plugin adapters expose an RGBA framebuffer and frame/trace summary. Core
   snapshots capture native visible state and restore by deterministic input
replay; plugin snapshots additionally bind the cartridge and frozen GlassVM
request to its scheduling cursor and lifecycle phase. Replay-envelope v3
serializes `fresh`, `incremental`, or `terminal` directly and integrity-binds
the complete payload. Fresh restores keep batch-execution authority,
incremental restores must extend the current input history, and terminal or
failed sessions remain forensic until reset.

Session execution is one-shot and fail-closed. Incremental stepping, immediate
input, or restoration selects incremental mode and makes later batch execution
invalid. Batch execution and failed steps are terminal; only reset re-arms the
session. Snapshot reads remain available in terminal mode for forensic evidence.

## Dependency boundary

TIC-80 and PICO-8 share `mlua` 0.10.5 with `lua54`, `vendored`, and `send`
features. They execute in one registry process, so using one binding prevents
Cargo's native `links = "lua"` conflict and avoids linking two incompatible
libraries that export the same Lua C ABI. Because upstream TIC-80 targets Lua
5.3, the descriptor exposes this choice as a non-native compatibility tier.

The machine host is Rust: chunk decoding, memory, input, graphics, APIs,
scheduling, limits, and snapshots contain no Lua implementation scripts. Lua
source remains an artifact payload, and a Lua interpreter remains necessary for
the declared Lua-cartridge variant. The pure-Rust VM assessment is documented
in the history record and intentionally not treated as a mechanical rewrite.

## Fidelity boundary

This is a deterministic bounded Lua compatibility runtime, not the complete
upstream fantasy console. Non-Lua engines, synthesized audio, scanline,
border, and overlay callbacks, texture-mapped triangles, exact system-font
rendering, and instruction-level Lua tracing remain outside v1. Sandboxing and
resource ceilings can also reject programs accepted by an unconstrained
native runtime, so the contract does not claim full native semantics.
