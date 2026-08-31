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
- `pico8_plugin` declares the GlassVM contract, adapts native reports and
  events, and owns session requests, independent evidence channels, and
  version-2 state-only continuation snapshots.

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
6. Plugin adapters emit the negotiated normalized events, native evidence,
   frame artifacts, input-value evidence, capabilities, and execution summary
   through their independent GlassVM channels.

## State boundary

Core snapshots record native runtime state. Plugin snapshots are version-2
continuation artifacts bound to the exact cartridge bytes and canonical frozen
`ExecutionRequest`. They contain only resumable machine state and the
scheduled-input cursor; they do not contain frame history, trace collections,
print history, or other observational aggregates.

Restore rejects incompatible versions, formats, request identities, and
invalid cursors before mutating the session. The PICO-8 runtime restores its
serialized machine state directly. No observational history is written to the
snapshot.

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
