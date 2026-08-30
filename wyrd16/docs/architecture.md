# Wyrd-16 architecture

The artifact is an aligned sequence of big-endian 16-bit runes loaded at byte
zero of a mutable 4 KiB circular arena. One rune executes per cycle through an
aligned 12-bit byte program counter. Drawing runes update a persistent 64×64
canvas; arithmetic, addressing, and control flow wrap deterministically.

`wyrd16_core` is the executable source of truth. Its artifact validator,
instruction decoder, state, transition engine, native input/output, and native
snapshot API have no GlassVM dependency. `wyrd16_verifier` depends on that core
for authoritative rune and artifact facts while retaining a tolerant artifact
view for useful malformed-input diagnostics.

`wyrd16_plugin` exposes one `MachineBundle`. It converts native reports and
effects into GlassVM capabilities and evidence, applies resolved body input,
and owns the session-bound snapshot envelope. That envelope binds the ROM and
request while retaining the complete native state, metrics, stimulus cursor,
event sequence, and partial-frame phase.

The canonical disk gate is `plugin/tests/bundle.rs`. It loads the drawing
`fixtures/smoke.rom`, runs the public execution/observation path,
checks snapshot integrity and exact restore/reset, and proves sink rejection
is visible.
