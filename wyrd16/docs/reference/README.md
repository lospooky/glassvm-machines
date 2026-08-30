# Wyrd-16 reference index

The normative specification is
[`docs/reference/wyrd16`](../../../../docs/reference/wyrd16/README.md).
The machine overview is [`../../README.md`](../../README.md), and the
implementation mapping is [`../implementation.md`](../implementation.md).

A v1 ROM is 1–2048 aligned big-endian 16-bit runes, limited by the 4 KiB
arena, with no wrapper or metadata. The canonical on-disk drawing program,
provenance record, and deterministic offline builder are in `../../fixtures`.

`sources.toml` identifies the external normative reference. `SHA256SUMS`
protects only the local reference corpus extension zone.
