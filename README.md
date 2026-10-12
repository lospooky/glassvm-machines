# GlassVM machine bundles

This repository provides three machine bundles for the [GlassVM runtime](https://github.com/lospooky/glassvm): CHIP-8, PICO-8, and TIC-80. Each bundle provides a Rust implementation and a Python provider that the generic `glassvm` facade discovers through package entry points.

## Install

Install the facade and all three providers with the tested selector:

```sh
uv add 'glassvm-machines[all]'
```

Install an individual provider with `uv add 'glassvm-machines[chip8]'`, `uv add 'glassvm-machines[pico8]'`, or `uv add 'glassvm-machines[tic80]'`. For direct dependency control, install `glassvm` and a provider such as `glassvm-chip8` separately. Python 3.12 or newer is required.

The `glassvm-machines` package contains dependency metadata only. It selects provider distributions; provider discovery remains the responsibility of `glassvm.machine_bundles` entry points.

## Supported machines

| Bundle | Artifact | Runtime boundary |
| --- | --- | --- |
| [CHIP-8](chip8/README.md) | Raw CHIP-8-family ROM | CHIP-8, CHIP-48, Super-CHIP, and XO-CHIP profiles. Optional native CUDA batch evaluation requires a compatible NVIDIA driver. |
| [PICO-8](pico8/README.md) | `.p8`, `.p8.png`, and `.p8.rom` cartridges | Deterministic headless compatibility runtime; Lua 5.4 and a documented API subset, without audio waveform synthesis. |
| [TIC-80](tic80/README.md) | `.tic` cartridge | Bounded Lua 5.4 compatibility runtime; non-Lua runtimes and several upstream features are outside the supported subset. |

The bundle guides describe supported behavior and limitations in detail. These implementations make no claim of complete compatibility with proprietary or upstream runtimes.

## Workspace

Each machine has three Rust crates: `core` owns native artifact handling and execution, `verifier` performs non-executing analysis, and `bundle` adapts the machine to GlassVM contracts. The machine's `python/` crate builds its provider extension. Shared GlassVM contracts and the generic Python facade live in the [GlassVM repository](https://github.com/lospooky/glassvm).

The Rust workspace also includes cross-machine conformance checks. From the repository root, format and test it with:

```sh
cargo fmt --all -- --check
cargo test --workspace --locked --no-fail-fast
```

See [development and validation](docs/development.md) for Python package builds and the installed-wheel smoke check.

## Documentation

- [Documentation index](docs/README.md)
- CHIP-8: [architecture](chip8/docs/architecture.md), [implementation](chip8/docs/implementation.md), and [testing](chip8/docs/testing.md)
- PICO-8: [architecture](pico8/docs/architecture.md), [implementation](pico8/docs/implementation.md), and [testing](pico8/docs/testing.md)
- TIC-80: [architecture](tic80/docs/architecture.md), [implementation](tic80/docs/implementation.md), and [testing](tic80/docs/testing.md)
