# GlassVM machine bundles

This repository's release branch contains the publication bundles for CHIP-8,
PICO-8, and TIC-80.
Each bundle provides its Rust core, verifier, GlassVM bundle integration, smoke fixture, and Python
provider extension. The shared GlassVM contracts, recorder, query layer, and
generic Python facade live in [the standalone GlassVM repository](https://github.com/lospooky/glassvm).

CHIP-8 is the baseline/reference implementation. PICO-8 and TIC-80 are
independently designed external validation machines for the paper's three-
machine publication denominator. Hexwell and Wyrd-16 are preserved at the
`parked/hexwell-wyrd16` branch and are outside this release branch and its
publication denominator.

Each machine root is also the source-distribution boundary for its Python
provider. The root `pyproject.toml` includes the complete bundle source
closure, while the compiled extension remains in the `python/` subdirectory.
Each provider declares its tested GlassVM facade dependency and exposes one
`glassvm.machine_bundles` entry point.

The only crates.io targets are the five machine-neutral GlassVM contracts.
Machine-specific Rust packages are internal wheel build components and are
marked `publish = false`. Their sibling source paths are included in each
machine's complete Python source distribution. GlassVM dependencies resolve
from crates.io; there is no Git or local-path dependency on the separate
GlassVM repository. Release GlassVM first, then update the machine lockfile to
the published crate versions before releasing providers.

The facade, three providers, and selector support Python 3.12 or newer. Current
CI covers CPython 3.12, 3.13, and 3.14 on Ubuntu 24.04 x86_64. Build and test
the complete machine workspace with:

```bash
cargo test --workspace --locked --no-fail-fast

uv build --directory chip8 --no-sources
uv build --directory pico8 --no-sources
uv build --directory tic80 --no-sources
uv build --directory distribution --no-sources
```

The installed-wheel smoke harness is
`conformance/python_clean_room.py`. Run it from an isolated environment after
installing the generic facade and the selected local bundle wheels:

```bash
uv pip install 'glassvm==0.2.0'
uv pip install --no-index --find-links dist 'glassvm-machines[all]==0.2.0'
python conformance/python_clean_room.py \
  --machine-root "$PWD" \
  --output-root /tmp/glassvm-python-runs \
  --expect chip8 pico8 tic80 \
  --invalid-artifact pico8 tic80
```

Use `--expect` with no values for a base-only environment and `--reject` to
assert that a bundle is not installed. The harness checks installed entry
points, exact provider metadata, native-module imports, preparation, separate
execution/evidence/recorder/publication results, and the atomic published run.
`--invalid-artifact` is explicit because bundles may choose whether malformed
artifact bytes are rejected during preparation or at session construction.

The Python distributions are:

- `glassvm-chip8`
- `glassvm-pico8`
- `glassvm-tic80`
- `glassvm-machines` (metadata-only tested dependency selections)

The three machine distributions expose entry-point-discovered providers for
the generic `glassvm` facade. Install bundles directly or use the selector's
tested extras:

```bash
uv add glassvm glassvm-chip8
uv add 'glassvm-machines[all]'
```

The `glassvm-machines` distribution adds dependency metadata only. It contains
no provider or static registry.
