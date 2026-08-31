# TIC-80 GlassVM bundle implementation record

**Implemented:** 2026-07-30

## Scope

The bundle adds TIC-80 as a registered GlassVM architecture without changing
`glassvm_core` or `rom_refinery/`. The executable variant is deliberately named
`tic80-lua`; other upstream cartridge languages are analyzed and rejected
explicitly.

## Evidence sources

The offline reference set is recorded in
[`../reference/README.md`](../reference/README.md). It contains:

- all files in the official GitHub wiki at revision
  `3b6a1ff0d33625e933b57e8b802e16e6a8470802`;
- the official generated `https://tic80.com/learn` page retrieved 2026-07-30;
- selected source-tree documentation, licenses, public headers, and build notes
  at source revision `4aba09c98f1e5028b82765be1647677b08d35942`;
- `SHA256SUMS` covering every snapshotted reference file.

## Implemented boundaries

- `.tic` chunk parser and cartridge analyzer/verifier;
- shared Lua 5.4 compatibility VM built from vendored source by `mlua`;
- documented RAM, VRAM, framebuffer, palette, asset, gamepad, and 60 Hz frame
  model;
- drawing, sprite/map, memory, persistent-memory, flags, input, timing, trace,
  and lifecycle APIs listed in the machine README;
- GlassVM descriptor, contract, ports, emulator, native-event adapter,
  observable adapter, static analyzer, and verifier;
- frame-level normalized input/display/lifecycle evidence;
- deterministic frame-start stimuli and replay-by-input-history snapshots;
- registration beside CHIP-8 and discovery through `glassvm_py`.

## Bounded-execution repair

The runtime treats cartridge data and Lua programs as untrusted:

- input cartridges are capped at 16 MiB;
- source code is capped at 512 KiB before execution, including the expanded
  output of legacy zlib-compressed chunks;
- each Lua state has a 16 MiB memory limit;
- top-level cartridge code, `BOOT()`, and every `TIC()` callback receive an
  independent 1,000,000-instruction budget;
- Lua libraries are explicitly allowlisted, and code loading, protected-call
  recovery, collection control, filesystem/process/package access, coroutines,
  and the debug library are unavailable.

Adversarial tests cover oversized cartridges, compressed expansion bombs,
infinite loops at all three execution entry points, and a single-call oversized
allocation. Sink rejection is terminal until reset, and fitness-versus-
forensics observation plans are checked to leave the machine result and final
frame digest unchanged.

The body contract now reports `preserves_native_semantics = false`. The current
runtime intentionally omits upstream features and adds fail-closed resource
limits, so a native-semantics claim would be misleading.

`glassvm_py` continues to expose the TIC-80 descriptor through the registry, but
its CHIP-8-shaped `evaluate_rom` function now rejects every non-CHIP-8 machine
before constructing an evaluation. This prevents a TIC-80 request from being
silently interpreted with CHIP-8 parameters.

## Lua engine assessment

The integrated repair uses the same `mlua` 0.10.5 and vendored PUC-Rio Lua 5.4
runtime as PICO-8. Cargo rejects distinct `mlua-sys` versions in one dependency
graph because both declare and export the native Lua library, so a purported
Lua-5.3/Lua-5.4 version split is not composable. `mlua` statically links the
selected runtime from source:
<https://github.com/mlua-rs/mlua#feature-flags>.

The pure-Rust [piccolo](https://github.com/kyren/piccolo) VM has promising fuel
and memory-accounting mechanisms, but it is explicitly pre-1.0/experimental and
its compatibility record still lists missing or differing base, string, UTF-8,
and package behavior:
<https://github.com/kyren/piccolo/blob/master/COMPATIBILITY.md>. TIC-80
cartridges are Lua programs, so changing interpreters is a language-compatibility
change, not a mechanical dependency swap. Without a representative cartridge
conformance corpus, adopting it here would create a half-compatible runtime.
The current build is self-contained and needs no system Lua, but it is not an
all-Rust dependency graph because the interpreter is vendored C. The body
contract names a bounded Lua-5.4 headless compatibility tier rather than
claiming TIC-80's native Lua-5.3 semantics.

## Verification

Passed:

```text
cargo test --locked -p pico8_plugin -p tic80_plugin --all-targets
  PICO-8: 22 unit + 1 real-cartridge test passed
  TIC-80: 11 unit + 1 upstream-gecko cartridge test passed

cargo test --locked -p glassvm_registry --test layout
  all 15 canonical layouts, package names, workspace memberships,
  lifecycle surfaces, reference corpora, symlinks, and smoke digests passed

cargo test --locked --workspace --all-targets
  passed in a fresh target directory

cargo clippy --locked <15 primary plugins + core/registry/Python> \
  --all-targets --no-deps -- -D warnings
  passed

git diff --check
  passed
```

The older secondary `chip8_core` and `chip8_verifier` crates retain pre-existing
Clippy style debt when selected directly with every warning denied; their full
functional test suites pass, and the canonical CHIP-8 primary bundle passes
the strict primary-package lint gate.

## Explicit follow-up work

See the runtime-coverage section in `../README.md`. The important gaps are
non-Lua language engines, sound synthesis, scanline/border/overlay callbacks,
texture-mapped triangles, the map remap callback, exact system-font rendering,
and instruction-level Lua VM tracing.
