# TIC-80 GlassVM machine bundle

The bundle follows the canonical three-crate envelope: `tic80_core` owns native
artifact and execution truth, `tic80_verifier` owns non-executing static
reasoning, and `tic80_plugin` owns the GlassVM boundary. The initial executable
variant runs Lua cartridges through a bounded
**Lua 5.4 compatibility runtime**. It loads binary `.tic` cartridges, maps
their graphics/map/palette/flags assets into the documented 96 KiB address
space, executes `BOOT()` and `TIC()` at 60 Hz, exposes the core
framebuffer/memory/input/drawing API, and emits normalized
frame/input/display evidence. Upstream TIC-80 targets Lua 5.3, so the bundle
does not claim language-version-exact native semantics.

This is intentionally not described as full upstream language parity. Ruby,
JavaScript, Wren, Squirrel, Fennel, MoonScript, Scheme, Janet, Python, and WASM
cartridges are identified by analysis but rejected by the Lua runtime.

The complete vendored reference snapshot is under
[`docs/reference`](docs/reference/README.md).

## Runtime coverage

Implemented:

- binary `.tic` chunk parsing, including multi-bank source and legacy
  zlib-compressed source;
- bank-0 tiles, sprites, map, palette, cover screen, and sprite flags;
- 96 KiB RAM, two 16 KiB VRAM banks, Sweetie-16 defaults, and composited RGBA
  output;
- `BOOT()` and `TIC()` under a vendored Lua 5.4 compatibility runtime at a
  deterministic 60 Hz;
- `cls`, `pix`, `line`, `rect`, `rectb`, `circ`, `circb`, `tri`, `trib`,
  `print`, `spr`, `map`, `clip`, `vbank`, `mget`, `mset`, `fget`, `fset`,
  `peek*`, `poke*`, `memcpy`, `memset`, `pmem`, `btn`, `btnp`, `time`,
  `tstamp`, `trace`, and `exit`;
- deterministic frame-start gamepad stimuli and versioned state-only
  continuation snapshots.

The native runtime uses exactly one step per frame. Its optional `runtime`
machine parameter defaults to `lua` only when absent; unknown parameters,
non-string runtime values, and runtime names other than `lua` fail closed.

Accepted as deterministic no-ops pending synthesis/state modeling: `sfx`,
`music`, and `sync`. Keyboard and mouse queries currently return neutral input.
The texture-mapped triangle, custom-font, scanline/border/overlay callbacks,
audio synthesis, map remap callback, and non-Lua runtimes remain explicit
follow-up work. The bundle emits frame-level evidence; it does not claim
instruction-level visibility inside the Lua VM.

## Execution limits

Cartridges are untrusted input, so the compatibility runtime fails closed at
fixed boundaries:

- 16 MiB maximum input cartridge size;
- 512 KiB maximum Lua source size, enforced on both source chunks and the
  expanded output of legacy zlib-compressed source chunks;
- 16 MiB Lua heap limit;
- 1,000,000 Lua VM instructions for each of cartridge loading, `BOOT()`, and
  each `TIC()` callback;
- only the Lua base, table, string, UTF-8, and math libraries, with dynamic
  loading, protected calls, collection control, filesystem, process, package,
  coroutine, and debug facilities unavailable.

Hitting any size, memory, or instruction limit is an execution error. The
contract therefore reports `preserves_native_semantics = false`: this is a
bounded TIC-80 Lua compatibility runtime, not a claim of complete upstream
behavior.

## Lua engine decision

The bundle shares PICO-8's `mlua` 0.10.5 binding and statically linked,
vendored PUC-Rio Lua 5.4 runtime. A single binding/runtime is required because
PICO-8 and TIC-80 are composed into the same registry process, while Cargo and
the native linker cannot safely link two incompatible libraries that both
export the Lua C ABI. The runtime supplies its own library allowlist, memory
limit, and instruction hook.

A pure-Rust Lua VM was assessed but not substituted. The leading candidate,
[piccolo](https://github.com/kyren/piccolo), is explicitly experimental and
documents important compatibility gaps in base, string, UTF-8, and package
behavior. Replacing the mature shared runtime without a broad TIC-80 cartridge
conformance corpus would silently trade compatibility for an
implementation-language preference. A future switch should require corpus
parity for language behavior, TIC-80 APIs, deterministic evidence, and the
resource-limit adversarial tests.

## Build and test

```sh
cargo test --locked -p tic80_core -p tic80_verifier -p tic80_plugin --all-targets
cargo clippy --locked -p tic80_core -p tic80_verifier -p tic80_plugin \
  --all-targets --no-deps -- -D warnings
```

The Lua interpreter is built from source through `mlua`'s vendored Lua 5.4
feature, so no system TIC-80 or Lua installation is required. The TIC-80 host
implementation is Rust, but the interpreter dependency includes vendored C;
this bundle therefore does not claim an all-Rust dependency graph.
