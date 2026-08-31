# PICO-8 bundle architecture

The bundle has three machine-owned crates with one-way dependencies:

```text
pico8_plugin -> pico8_verifier -> pico8_core
            `--------------------> pico8_core
```

Only `pico8_plugin` depends on GlassVM. This keeps native cartridge behavior
usable and testable without GlassVM and keeps static acceptance policy outside
the executable runtime.

## Components

- `pico8_core` identifies and decodes `.p8`, `.p8.png`, and `.p8.rom`
  cartridges. It owns cartridge Lua translation, the bounded `mlua` sandbox,
  the 64 KiB memory image, controller state, timing, graphics, effects, native
  events, and native runtime snapshots.
- `pico8_verifier` inspects source without executing callbacks. It reports
  artifact structure and capabilities, checks that translated Lua compiles,
  and diagnoses unresolved includes, unsupported APIs, and compatibility
  limits using native reports and diagnostics.
- `pico8_plugin` declares the GlassVM contract and body, adapts native reports,
  observations, and events, and owns execution requests, session-bound
  snapshots, normalized trace emission, and replay evidence.

## Execution flow

1. The codec validates the artifact envelope and source or binary payload.
2. The native verifier's analyzer reports format, version, callbacks, source
   sections, and referenced API groups.
3. The native verifier compiles translated Lua without running it, rejects
   unresolved includes, and reports unsupported APIs and the non-native
   numeric model.
4. A new native runtime initializes cartridge memory and deterministic random
   state,
   installs the compatibility API, loads Lua, and calls `_init`.
5. Each frame applies controller stimuli, runs `_update60` or `_update`, runs
   `_draw`, and captures framebuffer and callback evidence.
6. Plugin adapters emit a terminal result with replay manifest, receipt,
   package, state and trace fingerprints, framebuffer, hashes, and execution
   summary.

## State boundary

Core snapshots record native runtime state. Plugin snapshots are versioned
continuation artifacts bound to the exact cartridge bytes and canonical frozen
`ExecutionRequest`. They additionally record completed-frame scheduling,
ordered input history and its applied-input cursor, clean/advanced execution
phase, any terminal runtime error, and a domain-separated integrity digest over
the complete continuation payload.

Restore does not serialize Lua implementation objects. It constructs a fresh
runtime from the original cartridge, seed, and instruction budget, replays the
recorded inputs and callback frames, reproduces an optional failing callback,
and requires the rebuilt runtime image to equal the captured image before
transactionally replacing the active runtime. Closures and upvalues are
therefore reconstructed by execution. Cross-cartridge, cross-request,
noncanonical, unknown-field, corrupt-state, and cursor-inconsistent snapshots
fail without mutating the target session.

`step_frame` consumes the same frame-start stimulus schedule as full
`execute`, so manual continuation and normal runs have one input policy. Reset
still reconstructs the exact post-load state from the original artifact and
seed.

## Fidelity boundary

The implementation is deterministic but not bit-identical to the proprietary
PICO-8 runtime. It uses Lua 5.4 and host floating-point arithmetic, implements
an explicit graphics/input/memory/audio-command subset, and provides no audio
waveform synthesis. The contract therefore does not claim complete native
semantics.
