# Wyrd-16 machine

Wyrd-16 is a fantasy 16-bit rune computer built for evolutionary ROM search.
Its normative specification lives in
[`docs/reference/wyrd16`](../../docs/reference/wyrd16/README.md).

The canonical bundle has three intentionally one-way layers:

- `wyrd16_core` owns the reusable native artifact, state, instruction,
  execution, input/output, timing, and snapshot semantics;
- `wyrd16_verifier` owns non-executing analysis and acceptance diagnostics;
- `wyrd16_plugin` translates those native APIs into GlassVM contracts, bodies,
  sessions, trace evidence, adapters, and replay-bound snapshots.

```bash
cargo test -p wyrd16_core -p wyrd16_verifier -p wyrd16_plugin
cargo clippy -p wyrd16_core -p wyrd16_verifier -p wyrd16_plugin \
  --all-targets --no-deps -- -D warnings
```

The smallest useful drawing ROM is ten bytes:

```text
10 0A 11 14 B0 13 C0 10 00 01
```

It places a color-3 pixel at `(10,20)` and halts.

The checked-in executable copy and deterministic offline builder live under
`fixtures/`.
