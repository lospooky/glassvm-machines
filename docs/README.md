# GlassVM machine documentation

Start with the repository [README](../README.md) for installation, supported machines, and workspace commands. This page maps the deeper technical guides and explains how the repository is organized.

## Machine guides

Each machine has a landing page and detailed architecture, implementation, and testing documentation. The detailed guides are maintained alongside the machine code.

| Machine | Overview | Architecture | Implementation | Testing | References |
| --- | --- | --- | --- | --- | --- |
| CHIP-8 | [README](../chip8/README.md) | [Architecture](../chip8/docs/architecture.md) | [Implementation](../chip8/docs/implementation.md) | [Testing](../chip8/docs/testing.md) | [Reference provenance](../chip8/docs/reference/README.md) |
| PICO-8 | [README](../pico8/README.md) | [Architecture](../pico8/docs/architecture.md) | [Implementation](../pico8/docs/implementation.md) | [Testing](../pico8/docs/testing.md) | [Reference provenance](../pico8/docs/reference/README.md) |
| TIC-80 | [README](../tic80/README.md) | [Architecture](../tic80/docs/architecture.md) | [Implementation](../tic80/docs/implementation.md) | [Testing](../tic80/docs/testing.md) | [Reference provenance](../tic80/docs/reference/README.md) |

The machine directories also retain detailed implementation histories for [CHIP-8](../chip8/docs/history/README.md), [PICO-8](../pico8/docs/history/README.md), and [TIC-80](../tic80/docs/history/README.md), alongside fixture provenance and third-party source notes. These records provide additional machine-specific context; the current architecture, implementation, and testing guides describe supported project behavior.

## Repository layout

- `chip8/`, `pico8/`, and `tic80/` contain the machine core, verifier, GlassVM adapter, Python provider, fixtures, and machine-specific documentation.
- `conformance/` contains the shared Rust conformance checks and the installed Python provider smoke harness.
- `distribution/` builds the metadata-only `glassvm-machines` package, whose extras select tested provider combinations.
- `docs/development.md` explains workspace checks, provider builds, and the installed-wheel smoke check.

## Bundle boundaries

The core crate owns native artifact parsing and execution. The verifier performs static analysis without running the artifact. The bundle crate maps those machine-owned APIs to GlassVM's common contracts, session lifecycle, snapshots, and evidence channels. The Python extension exposes each bundle as a `glassvm.machine_bundles` entry point. Shared runtime contracts and the generic facade are maintained in the [GlassVM repository](https://github.com/lospooky/glassvm).

Each bundle documents its own fidelity boundary. In particular, a deterministic compatibility runtime does not imply complete equivalence with an upstream or proprietary runtime.
