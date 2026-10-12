# GlassVM machine selections

This metadata-only distribution selects tested GlassVM provider combinations.
It adds no runtime module, machine implementation, plugin mechanism, or
compiler. Provider discovery continues to use installed
`glassvm.machine_bundles` entry points.

Install one tested machine selection:

```bash
uv add 'glassvm-machines[chip8]'
uv add 'glassvm-machines[pico8]'
uv add 'glassvm-machines[tic80]'
uv add 'glassvm-machines[all]'
```

For direct dependency control, install the facade and provider distribution
separately, for example `uv add glassvm glassvm-chip8`.
