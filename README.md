# GlassVM machine bundles

This repository contains the publication bundles for CHIP-8, Hexwell, Wyrd-16,
PICO-8, and TIC-80.
Each bundle provides its Rust core, verifier, plugin, smoke fixture, and Python
provider extension. The shared GlassVM contracts, recorder, query layer, and
generic Python facade live in [the standalone GlassVM repository](https://github.com/lospooky/glassvm).

CHIP-8 is the baseline/reference implementation. PICO-8 and TIC-80 are
independently designed external validation machines for the paper's primary
publication denominator. Hexwell and Wyrd-16 remain fully supported, polished
clean-contract bundles used as adversarial/internal validation specimens. All
five use the same entry-point protocol and shared file-backed lifecycle.

Each machine root is also the source-distribution boundary for its Python
provider. The root `pyproject.toml` includes the complete bundle source
closure, while the compiled extension remains in the `python/` subdirectory.

The bundle crates consume the published `0.1.0` GlassVM contracts from
crates.io. Build and test
the complete machine workspace with:

```bash
cargo test --workspace --locked --no-fail-fast

uv build --directory chip8 --no-sources
uv build --directory hexwell --no-sources
uv build --directory wyrd16 --no-sources
uv build --directory pico8 --no-sources
uv build --directory tic80 --no-sources
```

The installed-wheel smoke harness is
`conformance/python_clean_room.py`. Run it from an isolated environment after
installing the generic facade and the selected local bundle wheels:

```bash
python conformance/python_clean_room.py \
  --machine-root "$PWD" \
  --output-root /tmp/glassvm-python-runs \
  --expect chip8 hexwell pico8 tic80 wyrd16 \
  --invalid-artifact pico8 tic80
```

Use `--expect` with no values for a base-only environment and `--reject` to
assert that a bundle is not installed. The harness checks installed entry
points, exact provider metadata, native-module imports, preparation, separate
execution/evidence/recorder/publication results, and the atomic published run.
`--invalid-artifact` is explicit because bundles may choose whether malformed
artifact bytes are rejected during preparation or at session construction.

The Python distributions are:

- `glassvm-machine-chip8`
- `glassvm-machine-hexwell`
- `glassvm-machine-wyrd16`
- `glassvm-machine-pico8`
- `glassvm-machine-tic80`

They expose entry-point-discovered providers for the generic `glassvm_py`
facade; bundle selection is handled by installing the desired distribution.
