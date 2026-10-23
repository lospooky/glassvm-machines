# Align visual capabilities with CHIP-8

Decision: retain CHIP-8's broader bounded visual-trajectory profile as the
reference for now. The proposed frame-artifact-only narrowing is superseded.
This document records the approved alignment and its implementation below.

## Approved conceptual model

```text
frame artifacts + selected normalized frame/display/input facts
    -> bounded online visual-trajectory reducer
    -> derived capability output + capability receipt
```

A visual capability may intentionally combine related evidence. Keep the
existing canonical IDs. Do not replace these profiles with frame-only
capabilities or remove existing fields/prerequisites merely to shrink requests.
The lifecycle remains `begin -> observe* -> finalize`, retaining scalar/summary
state and a previous frame digest, never frame/event history.

## Capability profiles

| Capability | Current facts summarized | Current prerequisites |
|---|---|---|
| `chip8.visual_trajectory_motifs` | Frame-boundary/artifact counts, changed/repeated fingerprints, full-artifact count, display-write events/state-write counts, maximum display writes per event, input-sample events | `FrameCompleted`, `DisplayWrite`, `InputSampled`, frames |
| `pico8.visual.motifs` | Frame-boundary count, changed/repeated fingerprints, graphics API calls, applied-input events | `FrameCompleted`, `InputApplied`, `glassvm.graphics_activity`, frames |
| `tic80.visual.motifs` | Frame-boundary count, changed/repeated fingerprints, graphics API calls, applied-input events, frame-level input samples, trace-event count | `FrameCompleted`, `InputApplied`, `InputSampled`, `glassvm.graphics_activity`, `tic80.trace`, frames |

Frames means hashes, with full artifacts also satisfying that prerequisite.
Current dependencies are honest for the actual outputs. In particular, removing
TIC-80's input/trace dependencies without changing its fields would be wrong.

## Alignment boundaries

Use CHIP-8 as the richness/lifecycle reference, not as permission to invent
equivalent facts on another substrate:

- Frame boundaries, captured artifacts, and fingerprint transitions remain
  separate counts.
- Full-artifact count describes capture richness, not visual behavior.
  Hash/full capture must agree on comparable fingerprint-change metrics;
  explicitly capture-specific accounting may differ.
- PICO-8/TIC-80 `glassvm.graphics_activity` counts drawing API calls, including
  clipped/empty attempts. Those are not CHIP-8 display-state writes, pixel
  changes, or counts of emitted `StateWrite` records.
- `InputApplied` counts application facts, not input reads. PICO-8's visual
  capability reports applied inputs; it does not claim to measure cartridge
  `btn`/`btnp` reads. TIC-80 retains its frame-level `InputSampled` count, which
  also does not identify individual Lua reads.
- Frame-level input sampling must not be called actual `btn`/`btnp` consumption
  unless that distinction is observed.
- Retain TIC-80's existing trace field and its prerequisite for now.

## Implemented alignment

Both fantasy-console visual reducers now consume the already-emitted bounded
`glassvm.graphics_activity` event and normalized `InputApplied` facts, alongside
frame boundaries and first-class frame artifacts. The output fields are
`graphics_api_calls` and `applied_inputs`, retaining the source units explicitly.
TIC-80 continues to output its existing `sampled_inputs` and `trace_events`.
The two visual capability schemas are now `2.0.0`; their canonical IDs are
unchanged. Catalog prerequisites select every event the reducers consume.

This aligns the visual profile's evidence breadth with CHIP-8 while preserving
machine-specific meanings. It does not claim equivalent display-write or
button-read instrumentation. The paper workload manifest selects the expanded
prerequisites; historical measurements remain untouched.

Do not force identical keys where the units/meanings differ. A common
cross-machine output schema or renamed capability IDs would require an
additional explicit decision.

## Implementation sequence

1. [x] Map enriched fields to existing selected events and define their units.
2. [x] Update bounded reducers, output schema versions, and exact prerequisites
   together. Do not add derived capabilities solely for catalog symmetry.
3. Preparation resolves the prerequisite closure. Required missing inputs fail
   preparation; optional failures remain explicit unavailability. No runtime
   discovery of missing inputs.
4. [x] Numerically version output-semantic changes while keeping IDs
   canonical/unversioned. No aliases, fallback readers, or dual reducers. The
   withdrawn frame-only proposal does not itself mandate a version bump.
5. [ ] Verify exact counters, required/optional negotiation, bounded state,
   hash/full comparable metrics, and catalog/output/receipt schema agreement.
6. [ ] Run small Python-consumer + Rust-recorder checks and update paper plans.
   Preserve independent execution/evidence/recorder/publication outcomes.
   Released 0.3.0 artifacts remain unchanged; release pins and affected final
   regeneration remain later gates.

CHIP-8 is not being simplified. The existing activity evidence remains
independently selectable and does not automatically become a new capability.
