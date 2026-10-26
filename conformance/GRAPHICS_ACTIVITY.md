# Selective normalized graphics activity

Subsequent work: [`RUNTIME_ACTIVITY.md`](RUNTIME_ACTIVITY.md) adds separate
callback/hook and RNG families and fixes TIC-80 RNG initialization. The
measurements below describe the graphics slice alone, not that later build.

Implemented locally on `paper-publication`, 2026-10-21. This is a follow-up to
the released 0.3.0 evidence, not a change to those sealed measurements. Both
provider wheels used below are unreleased source builds retaining 0.3.0 package
metadata. Release/pin updates and affected paper regeneration remain necessary.

## Why this comes first

The original 600-frame paper requests selected almost everything PICO-8 and
TIC-80 exposed as normalized events. PICO-8 emitted 600 frame boundaries plus
two lifecycle events. TIC-80 additionally emitted 600 frame-level input samples.
The selected recipes removed only the lifecycle events. No capability expanded
either request; access/detail filters were disabled. Filtering worked, but the
available normalized vocabulary omitted the drawing work driving these carts.

This slice adds one meaningful, independently selectable family. It does not
claim instruction-level Lua visibility or complete CHIP-8 observability parity.

## Contract

Event kind: `EventKind::Extension("glassvm.graphics_activity")`.
Both bundle crates export `GRAPHICS_ACTIVITY` for that ID.

The normalized event's `extensions["glassvm.graphics_activity"]` contains:

```json
{
  "schema": {
    "id": "glassvm.graphics_activity",
    "version": { "major": 1, "minor": 0, "patch": 0 }
  },
  "draw_calls": 159
}
```

`draw_calls` is a positive unsigned integer: the number of decoded invocations
of covered drawing APIs during this successfully completed frame. Clipped,
transparent, degenerate, empty, or otherwise pixel-neutral attempts still
count. Pixel reads do not count. This is not a changed-pixel count, motion
score, complexity measure, rendered-output equivalence claim, or count of
internal pixels/spans/tiles.

At most one event is emitted per completed frame. No drawing calls means no
event, not a zero-filled placeholder. Calls during loading/initialization are
excluded. Calls from frame callbacks count, including TIC-80 `BDR` and drawing
APIs invoked by a `map` remap callback. A map invocation counts once; its
internal tile draws do not count separately. Explicit nested drawing API calls
count separately. Failed frames produce no completed-frame activity summary;
this family is not a partial-execution diagnostic stream.

`frame` is the zero-based semantic frame just completed; `step` follows the
bundle's existing frame-step convention. Both match that frame's
`FrameCompleted` coordinates when selected together. Sequence allocation is
unchanged and is not a cross-channel merge key or semantic frame identity.

The summary is selected independently from `FrameCompleted`, native evidence,
and frame artifacts. `All` includes it; an exact `Kinds` request can select or
omit it. Existing broader `Effects`/`Instructions` selectors include extensions
under their existing rules; that does not imply Lua instruction tracing.
Normalized-event budgets account for it through the unchanged `BudgetedSink`.
Evidence truncation does not redefine machine execution success.

Python exact selection, within an otherwise valid observation request:

```python
observation["normalized_events"]["events"] = {
    "kinds": [{"extension": "glassvm.graphics_activity"}]
}
```

The existing normalized-event envelope/payload mechanism and Python bridge are
used unchanged. No new core emission variant, native kind, frame schema,
capability, recorder format, or derived interpretation is introduced.

### Covered APIs

| Bundle | Counted drawing calls |
|---|---|
| PICO-8 | `cls`, `pset`, `rectfill`, `rect`, `line`, `spr`, `sspr` |
| TIC-80 | `cls`, `pix` setter, `line`, `rect`, `rectb`, `circ`, `circb`, `tri`, `trib`, `ttri`, `spr`, `map` |

This is deliberately a scoped API-activity summary, not a census of framebuffer
mutation. Direct memory writes/copies, palette/camera/clip/bank changes, and
the current print/log/size-return APIs are excluded. There is no claim that
every graphics-related API has the same semantics across machines.

## Bounded implementation

PICO-8 takes the difference between two scalar reads of its existing cumulative
drawing counter. Those reads and normalized-payload construction occur only
when the prepared selection includes this family. Per-frame calls are bounded
by the existing instruction budget; subtraction handles cumulative wrapping.
The underlying native counter already existed and remains independent of this
observation request.

TIC-80 resolves a capture flag once from the prepared request. Its runtime uses
one optional scalar counter reset for each tick and consumed on completion.
Unselected calls do not accumulate a counter or allocate graphics records;
the API-entry gate still has a small branch cost. The counter saturates
defensively; the frame instruction budget bounds the attainable count. There
is no per-pixel instrumentation, API-call vector, or completed-frame history.

Instrumentation does not enter resumable snapshots. Stepping/reconstruction
does not emit historical activity; resumed execution counts only newly
completed frames. Existing visual reducer prerequisites and outputs remain
unchanged: those reducers do not consume this new family.

## Spot evidence

Artifacts live at:
`/home/spooky/workspace/glassvm-paper-results/graphics-observation-20261021/`.
`before/` uses the preceding performance-sanitation build; `after/` adds this
graphics slice. Each matrix has two machines, three recipes, 600 frames, and
three independent worker repetitions: 18 trials, not the full paper suite.
All 36 trials completed execution, evidence, recorder finalization, and atomic
publication successfully. Both matrices use the existing Rust file recorder.
Fixture hashes and requests are retained in the raw JSON.

| Bundle | All events before → after | Selected events | Reduction within the new build | All encoded bytes after | Selected encoded bytes |
|---|---:|---:|---:|---:|---:|
| PICO-8 | 602 → 1,202 | 600 | 50.08% | 416,189 | 168,080 |
| TIC-80 | 1,202 → 1,802 | 1,200 | 33.41% | 616,933 | 374,240 |

The new denominator contains useful additional evidence; this is not a repair
to the filtering predicate and does not retroactively improve released results.
The original selected recipes remain unchanged. A graphics-only request
produces 600 summaries for each cart: 1,518,196 covered calls in PICO-8
(257–2,673 per frame), and 95,400 in TIC-80 (159 per frame).
`projections.json` proves the frame/step/payload projection is identical between
graphics-only and all-normalized requests, with no native/frame channel.

Median execution + finalization/publication time, milliseconds:

| Bundle | Request | Before | After |
|---|---|---:|---:|
| PICO-8 | Disabled | 397.639 | 396.024 |
| PICO-8 | All normalized | 427.774 | 436.473 |
| PICO-8 | Selected normalized | 422.637 | 420.004 |
| TIC-80 | Disabled | 149.864 | 146.974 |
| TIC-80 | All normalized | 187.299 | 208.489 |
| TIC-80 | Selected normalized | 188.640 | 183.036 |

These short local medians are descriptive, not a speedup/significance claim.
Rendering still dominates, especially in PICO-8; fewer records do not imply a
proportionate runtime reduction. More requested evidence has a real cost.

The 20-case short evidence probe also ran. All 16 cases excluding the new
family match the prior optimized build exactly: channel fingerprints, machine
outcomes, capability/evidence receipts, and recorder accounting. The four
`All`/all-evidence cases intentionally change. Their extra records and sequence
allocations are not suppressed to manufacture byte-equivalence.

### Reproduce the spot checks

Install freshly built local provider wheels into a dedicated environment;
do not identify these builds solely by their unchanged distribution versions.
Preserved wheels are under `builds/{before,after}` in the artifact directory.
Then, from the machines checkout:

```sh
python conformance/graphics_activity_probe.py \
  --paper-root ../glassvm/docs/paper \
  --output /chosen/result-dir/projections.json
```

For the limited timing matrix, from the GlassVM checkout:

```sh
python docs/paper/experiments/run_pilot.py \
  --machines-root ../glassvm-machines \
  --output-dir /chosen/result-dir/matrix \
  --machines pico8 tic80 \
  --recipes disabled all_normalized selected_normalized \
  --frame-limit 600 --repetitions 3
```

Rust verification: the PICO-8/TIC-80 core and bundle suites plus machine
conformance pass. Four new shared tests cover exact schema/counts, independent
selection and unchanged frame content, state-only continuation, and hard
normalized-budget accounting. A native TIC-80 test checks opt-in counters,
empty frames, BDR/remap calls, and identical machine snapshots/RGBA.
Clippy with denied warnings and formatting checks pass for the touched crates.

## Remaining prioritized gaps

1. Callback/computation/RNG activity: expose real bounded execution facts beyond
   frame timing without pretending to trace Lua bytecode.
2. Capability granularity: add only useful reducers with exact dependencies;
   do not broaden existing visual prerequisites to force this family in.
3. Actual input consumption: distinguish frame-level polling summaries from
   meaningful `btn`/`btnp` consumption facts.
4. Selective state access/diffs: explicit ranges/kinds and bounded production,
   not unconditional memory snapshots or a hidden trace store.

Audio synthesis, full native fidelity, debug visibility, and bytecode tracing
remain separate scope. No further reducer or core redesign is part of this slice.
