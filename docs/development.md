# Development and validation

These commands assume the repository root as the working directory. The workspace requires Rust; Python provider builds require Python 3.12 or newer and `uv`.

## Rust workspace

Check formatting and run the full Rust workspace and conformance suite:

```sh
cargo fmt --all -- --check
cargo test --workspace --locked --no-fail-fast
```

For focused machine work, the [machine testing guides](README.md#machine-guides) list crate-specific commands and any optional hardware requirements. CHIP-8 CUDA device tests are opt-in; ordinary workspace checks do not require a CUDA device.

## Build Python distributions

Build the three providers and metadata-only selector into a shared output directory:

```sh
mkdir -p dist
uv build --directory chip8 --no-sources --out-dir dist
uv build --directory pico8 --no-sources --out-dir dist
uv build --directory tic80 --no-sources --out-dir dist
uv build --directory distribution --no-sources --out-dir dist
```

Each provider's source distribution includes its Rust source closure. The provider extension is compiled when building its wheel. The selector package contains only its project metadata and README; its optional dependencies choose provider packages.

## Check installed wheels

The clean-room harness checks installed entry points, native extension imports, preparation and execution, and output publication. It reads the smoke fixtures from this checkout but runs in an isolated Python environment.

After building the distributions above, create an environment and install the facade and local selector wheel:

```sh
uv venv /tmp/glassvm-machines-venv
uv pip install --python /tmp/glassvm-machines-venv/bin/python 'glassvm==0.3.0'
uv pip install --python /tmp/glassvm-machines-venv/bin/python \
  --no-index --find-links dist 'glassvm-machines[all]==0.3.0'
```

Run the harness from the repository root:

```sh
/tmp/glassvm-machines-venv/bin/python conformance/python_clean_room.py \
  --machine-root "$PWD" \
  --output-root /tmp/glassvm-python-runs \
  --expect chip8 pico8 tic80 \
  --invalid-artifact pico8 tic80
```

The `--invalid-artifact` option asserts that the named providers reject malformed input during preparation. The CHIP-8, PICO-8, and TIC-80 implementations can make different admission decisions, so the option is explicit. Use `--expect` with no values for a facade-only environment, or `--reject` to assert that selected providers are absent.

The CI workflow also builds provider wheels from their source distributions and checks base-only, per-provider, and all-provider installations across its supported Python versions. See [CI configuration](../.github/workflows/ci.yml) for that release-like build path.
