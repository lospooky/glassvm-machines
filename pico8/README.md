# PICO-8 GlassVM machine bundle

The PICO-8 bundle provides cartridge codecs and a deterministic headless
compatibility runtime for GlassVM. It accepts native text `.p8`, steganographic
`.p8.png`, and raw `.p8.rom` cartridge representations.

The bundle follows the canonical three-crate envelope. `pico8_core` owns native
cartridge decoding and deterministic execution, `pico8_verifier` owns
non-executing compatibility analysis, and `pico8_bundle` owns only GlassVM
integration. The runtime materializes the 64 KiB fantasy-console memory map,
runs translated cartridge Lua, drives `_init`, `_update` or `_update60`, and
`_draw`, and exposes the framebuffer, controller input, audio commands, random
source, GPIO, replay evidence, and strict restorable continuation snapshots.

This is explicitly a compatibility implementation. It uses Lua 5.4 host
numbers instead of PICO-8's exact numeric behavior, implements a documented
headless API subset, and observes audio commands without synthesizing a
waveform. Unsupported calls and self-contained-cartridge violations are
reported by the verifier.

Normalized drawing-call activity is independently selectable as
`Extension("glassvm.graphics_activity")`. It summarizes covered API calls per
completed frame, not changed pixels or visual complexity. Selection, shared
schema, and coverage limits are documented in
[`GRAPHICS_ACTIVITY.md`](../conformance/GRAPHICS_ACTIVITY.md).

Callback/budget-hook accounting and RNG API activity are independently
selectable as `glassvm.execution_activity` and `glassvm.rng_activity`.
They use bounded per-frame summaries, not Lua instruction traces or RNG values;
see [`RUNTIME_ACTIVITY.md`](../conformance/RUNTIME_ACTIVITY.md).

## Build and test

```sh
cargo test --locked -p pico8_core -p pico8_verifier -p pico8_bundle --all-targets
cargo clippy --locked -p pico8_core -p pico8_verifier -p pico8_bundle \
  --all-targets --no-deps -- -D warnings
```

Architecture, implementation details, and reference provenance are in
[`docs`](docs/).
