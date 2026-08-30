# Hexwell implementation

Catalyst decoding and geometry live in `core/src/machine/instruction.rs`;
reactor state and sweep semantics live in `core/src/machine/runtime.rs`.
Static plate profiles and advisory rules are native verifier outputs. GlassVM
request, stimulus, trace, and snapshot identities remain confined to plugin.

The migration preserves `hexwell-semantics.v1`, all schema identifiers,
ordered sweep evidence, frame hashes, replay behavior, and exact continuation.
