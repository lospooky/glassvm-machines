# PICO-8 bundle implementation

## Cartridge codecs

Text cartridges require the PICO-8 header, a numeric version line, and a
`__lua__` section. Asset sections are decoded into their documented memory
regions. PNG cartridges must have the native 160x205 RGBA envelope; cartridge
bits are extracted from channel low bits and the stored payload hash is
checked when present. Raw cartridges use the 32 KiB binary payload form.

All codecs fail closed on malformed lengths, invalid hexadecimal content,
corrupt compressed source, or missing program source. The analyzer preserves
format and section metadata without executing the program.

## Runtime

`Pico8Runtime` owns the cartridge RAM, Lua state, callback schedule, packed
128x128 framebuffer, deterministic random generator, controller masks, draw
counts, audio-command log, and print output. The host API bounds memory and
coordinates and keeps callback execution under a configurable instruction
budget.

`_update60` selects 60 Hz callback behavior; otherwise `_update` selects the
30 Hz-compatible path. `_draw` runs after the selected update callback. Input
stimuli are frame-addressed and use a 16-bit mask for two six-button logical
controllers.

## Observation and replay

The plugin emits lifecycle, input, callback, effect, and completed-frame
events according to the observation plan. Capabilities include display
dimensions, flat palette-index framebuffer, frame hashes, and a compatibility
summary. Rich traces are marked filtered because callback/effect boundaries
are not a complete VM instruction stream.

Every run produces a replay manifest, receipt, and package with artifact,
machine, seed, budget, stimulus, final-state, trace, and report identities.

Continuation snapshots use canonical JSON with a strict versioned schema.
They bind SHA-256 identities for both the cartridge and canonical execution
request, retain the recorded controller history and consumption cursor, and
include an integrity digest plus an expected observable runtime image as
replay validation evidence.
Restore builds a fresh sandbox, replays successful frames and an optional
terminal failing frame, compares the complete runtime image, and commits the
replacement only after every check succeeds. Hidden Lua closures and upvalues
are reconstructed by deterministic replay rather than serialized.

Manual `step_frame` applies scheduled frame-start stimuli before callbacks,
matching `execute`. Direct input changes are appended as already-applied
recorded stimuli only when no scheduled input remains pending. Runtime errors
and sink-aborted executions remain terminal until reset or restoration of an
explicit compatible continuation.

## Known limits

The verifier always warns that the numeric model differs from PICO-8's native
arithmetic. Calls beyond the installed API subset are surfaced as
compatibility warnings. Persistent cart data, menu/host commands, full
coroutine semantics, remote includes, and synthesized audio are outside the
v1 execution claim.
