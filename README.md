# GlassVM machine bundles

This repository contains the publication bundles for CHIP-8, Hexwell, Wyrd-16,
PICO-8, and TIC-80.
Each bundle provides its Rust core, verifier, plugin, smoke fixture, and Python
provider extension. The shared GlassVM contracts, recorder, query layer, and
generic Python facade live in [the standalone GlassVM repository](https://github.com/lospooky/glassvm).

PICO-8 and TIC-80 are additional clean-contract bundles. Their provider wheels
use the same entry-point protocol and shared file-backed lifecycle as the three
reference publication bundles. The paper's reference-machine denominator still
remains CHIP-8, Hexwell, and Wyrd-16.

Each machine root is also the source-distribution boundary for its Python
provider. The root `pyproject.toml` includes the complete bundle source
closure, while the compiled extension remains in the `python/` subdirectory.

The bundle crates consume the tagged `v0.1.0` GlassVM contracts. Build and test
the complete machine workspace with:

```bash
cargo test --workspace --locked --no-fail-fast

uv build --directory chip8 --no-sources
uv build --directory hexwell --no-sources
uv build --directory wyrd16 --no-sources
```

The Python distributions are:

- `glassvm-machine-chip8`
- `glassvm-machine-hexwell`
- `glassvm-machine-wyrd16`
- `glassvm-machine-pico8`
- `glassvm-machine-tic80`

They expose entry-point-discovered providers for the generic `glassvm_py`
facade; bundle selection is handled by installing the desired distribution.
