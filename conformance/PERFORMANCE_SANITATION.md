# PICO-8 and TIC-80 performance sanitation

Measured on 2026-10-21. These are short local source-build experiments, scoped
to the existing `fireintro.p8` and `game.tic` paper workloads on this host.
They are not a replacement for the published 0.3.0 paper evidence.

## Sources and retained artifacts

The baseline machine source is commit
`4ea3d25b00991d93c2171623d501856fd67eca01`. Both provider wheels were built
from that source before changing the implementation. Optimized providers were
built from the changes accompanying this report. The generic Python facade
and GlassVM contract/recorder dependencies are the published 0.3.0 packages.
Provider distribution metadata still says 0.3.0: the optimized wheels are
unreleased local builds, not the wheels currently on PyPI.

Results are retained outside both repositories:

```text
/home/spooky/workspace/glassvm-paper-results/performance-sanitation-20261021/
  baseline/                 original 600-frame observation recipes
  frame-gate/               early frame-capture guard only; one repetition
  selection/                scalar counters, lazy payloads, selected native events
  final/                    intermediate rectangle-span implementation
  optimized/                final circle/span and evidence-selection implementation
  baseline-null/            original 1,200-frame execution without a recorder
  final-null/               intermediate rectangle-span execution without a recorder
  optimized-null/           final execution without a recorder
  *-evidence.json           short before/after logical projection fingerprints
  short-equivalence/        eight-frame Python/Rust fan-out checks
  profiles/                 Callgrind instruction profiles
  builds/{baseline,optimized}/  retained provider wheels
  implementation.patch      tracked source changes used by the optimized build
```

Use `baseline` versus `optimized`, and `baseline-null` versus `optimized-null`,
for the before/after comparisons below. Intermediate stages preserve the
incremental measurements. The existing 0.3.0 final-run directories are retained.

## Root causes and changes

1. Both bundle frame emitters extracted the framebuffer before checking
   `FrameCapture::None`. They now return before extraction, unpacking, RGBA
   conversion, allocation, or hashing when frame capture is disabled.
2. PICO-8 cloned `RuntimeSnapshot`, including 64 KiB RAM and its bounded print
   log, every frame to read two counters. `Pico8Runtime::counters()` copies
   scalar values. The native selector guards this read, including the final
   execution-summary read, through lazy native-payload construction.
3. Both bundles constructed scheduled `InputApplied` metadata before checking
   normalized selection. They now check first. Input-value payloads remain
   independently selected by the prepared concrete input-ID set.
4. TIC-80 always allocated an outcome event buffer containing input/frame
   events. A native, machine-local `EventCapture` is derived from the prepared
   normalized/native selectors. Execution constructs only the required event
   objects. Stepping and snapshot reconstruction use an empty capture mask.
5. Native JSON payloads and TIC-80 normalized extensions were constructed
   before selection. Native payload closures are evaluated after selection;
   normalized extension construction follows its selector too.
6. TIC-80 copied each `trace()` argument into an owned Rust string even with
   trace capture disabled. It now validates borrowed text and owns a copy only
   for requested traces. Numeric coercion, UTF-8 validation, and cumulative
   trace counts remain active independently of observation.
7. Profiling identified TIC-80 filled circles as a large caller of per-pixel
   clipping/palette/memory work. Clipped packed row fills now serve filled
   rectangles, clipped clears, and integer circle spans. Partial-byte writes
   preserve neighboring pixels. Circle coverage uses integer square root;
   arithmetic overflow cases retain the existing execution behavior.

Frame schemas, fingerprint inputs, normalized/native payloads, coordinates,
sequence values, capability prerequisites, preparation identities, and recorder
format are unchanged. Snapshots are produced on their existing requested path.

## Measurements

Each observation row uses 600 frames and three separate worker processes per
machine and recipe, in the existing seeded shuffled order. The first guard-only
stage uses one repetition. Every observation recipe uses the Rust reference
file recorder, including the recipe requesting no evidence channels.

The separate no-recorder experiment uses 1,200 frames and three processes per
cell. `observed_no_external_sink` requests the existing visual capability and
its exact normalized-event/frame-hash prerequisites. It exercises the shared
execution path with no external Python consumer or file recorder.

Times are medians measured around `prepared.execute(...)`, including finalization
and publication where applicable. They exclude interpreter startup, discovery,
and preparation. Raw JSON retains those other timing boundaries. Ratios are
baseline time divided by optimized time. These short measurements describe
these workloads and this host; they do not rank ISA performance.

### No external recorder: 1,200 frames

| Machine | Observation | Before ms | After ms | Speedup | Before/after peak RSS MiB |
|---|---|---:|---:|---:|---:|
| PICO-8 | Disabled | 597.134 | 480.438 | 1.243× | 27.46 / 27.21 |
| PICO-8 | Visual capability + prerequisites | 637.384 | 533.512 | 1.195× | 27.70 / 27.44 |
| TIC-80 | Disabled | 211.868 | 97.494 | 2.173× | 26.18 / 26.22 |
| TIC-80 | Visual capability + prerequisites | 300.334 | 261.046 | 1.151× | 27.19 / 27.28 |

The disabled execution path uses about 19.5% less time for PICO-8 and 54.0%
less for TIC-80. Peak process RSS is similar; this experiment does not measure
allocator high-water usage or establish a new memory-scaling claim.

### Reference recorder: 600 frames

| Machine | Recipe | Before ms | After ms | Speedup |
|---|---|---:|---:|---:|
| PICO-8 | Disabled | 432.658 | 386.515 | 1.119× |
| PICO-8 | Lifecycle/minimal | 440.674 | 406.200 | 1.085× |
| PICO-8 | Selected normalized | 459.790 | 421.265 | 1.091× |
| PICO-8 | Visual capability | 489.415 | 462.652 | 1.058× |
| PICO-8 | Frame hashes | 461.295 | 434.846 | 1.061× |
| PICO-8 | Full frames | 1,644.469 | 1,653.453 | 0.995× |
| PICO-8 | Mixed | 483.086 | 463.148 | 1.043× |
| TIC-80 | Disabled | 204.648 | 149.727 | 1.367× |
| TIC-80 | Lifecycle/minimal | 210.881 | 151.006 | 1.397× |
| TIC-80 | Selected normalized | 245.637 | 190.000 | 1.293× |
| TIC-80 | Visual capability | 303.722 | 293.588 | 1.035× |
| TIC-80 | Frame hashes | 261.960 | 247.388 | 1.059× |
| TIC-80 | Full frames | 9,947.045 | 9,945.082 | 1.000× |
| TIC-80 | Mixed | 311.949 | 294.907 | 1.058× |

Full-frame recorder timing is effectively unchanged. Its evidence volume is
unchanged, and these optimizations do not alter the recorder implementation.
Differences of a few percent in the smaller improvements need more repetitions
before making strong quantitative claims.

The following channel totals are exactly equal before and after in all
42 matching benchmark cases. Logical bytes use the existing recorder accounting.
Lifecycle manifests/operational files are outside these channel-record counts.

| Machine | Recipe | Channel records | Logical bytes | Encoded bytes |
|---|---|---:|---:|---:|
| PICO-8 | Disabled | 0 | 0 | 0 |
| PICO-8 | Lifecycle/minimal | 2 | 712 | 545 |
| PICO-8 | Selected normalized | 600 | 217,960 | 168,080 |
| PICO-8 | Visual capability | 1,201 | 513,551 | 392,723 |
| PICO-8 | Frame hashes | 600 | 295,068 | 224,184 |
| PICO-8 | Full frames | 600 | 20,185,846 | 10,010,160 |
| PICO-8 | Mixed | 1,202 | 514,111 | 393,189 |
| TIC-80 | Disabled | 0 | 0 | 0 |
| TIC-80 | Lifecycle/minimal | 2 | 698 | 529 |
| TIC-80 | Selected normalized | 1,200 | 489,030 | 374,240 |
| TIC-80 | Visual capability | 1,801 | 774,839 | 588,116 |
| TIC-80 | Frame hashes | 600 | 284,848 | 213,385 |
| TIC-80 | Full frames | 600 | 257,035,067 | 120,220,554 |
| TIC-80 | Mixed | 1,802 | 775,270 | 588,464 |

Optimized median peak process RSS across these recipes is 28.40–28.71 MiB for
PICO-8 and 27.38–32.68 MiB for TIC-80. Raw trials retain per-recipe values.

## Correctness and equivalence

- Focused PICO-8 and TIC-80 core/bundle unit and integration suites pass.
  These include deterministic execution, Send preservation, sandbox/budget
  enforcement, typed frame capture, and snapshot restoration/continuation.
- The 23 targeted shared conformance tests pass for observation channels,
  precise capability prerequisites, derived outputs, input values, frame
  continuation, budgets, and durable publication. The three layout/prohibited
  surface checks pass too. Formatting and whitespace checks pass.
- All 42 original/final observation trials have exactly equal requests,
  workloads, prepared-observation IDs, machine results, evidence receipts,
  recorder receipts, and per-channel accounting. Every run publishes.
- `performance_evidence_probe.py` compares 20 short cases: both machines,
  all nine existing recipes, and an all-evidence case including final snapshot,
  native events, normalized events, full frames, and visual capability output.
  All per-channel fingerprints and receipts match the original providers.
  Lifecycle comparisons also cover both prepared identities. The diagnostic
  fingerprints are test comparisons, not a new GlassVM identity contract.
- Two eight-frame executions pass the existing Python/Rust fan-out comparison:
  exact prepared-run/observation identities, logical channel records, capability
  outputs/receipts, and evidence receipts match. The paper mixed recipe has no
  scheduled inputs or snapshots; input/snapshot cases are covered separately.
- New native tests prove that disabling TIC-80 instrumentation leaves state,
  frames, and trace counts unchanged while allocating no outcome-event buffer.
  Lazy-payload tests panic if an unselected native payload is evaluated.
- Differential graphics tests compare packed rectangles and integer circles
  with pixel-by-pixel writes across clipping, odd/even pixel boundaries,
  palette mappings, negative/zero sizes, screen edges, and both video banks.
  Both complete native snapshots and rendered bytes match.
- Callback mutation and trace-validation tests preserve dynamic Lua globals,
  numeric text coercion, valid Unicode, and errors for invalid UTF-8 even when
  observation is disabled.

The full paper suite and long-run memory matrix were not rerun. The retained
0.3.0 results continue to describe the original released implementation.

## Profiling and rejected changes

Valgrind 3.18.1 was extracted into a temporary directory, without a system
installation. Callgrind collected execution regions of 60-frame disabled runs.
The numbers below are executed instruction counts, not wall-clock percentages.

- PICO-8: `luaV_execute` accounts for 30.80% and `luaG_traceexec` for 22.25%
  of the collected instructions after the evidence-selection fixes. Host API
  conversion/callback work is also visible. The instruction hook enforces the
  configured budget; it remains enabled. Synchronization was retained because
  this profile does not isolate enough mutex cost to justify changing ownership
  or thread guarantees.
- TIC-80 before graphics changes: `set_pixel` accounts for 32.65%; the caller
  graph identifies the filled-circle callback. After span filling, collected
  instructions fall from 132,118,378 to 88,413,060, about 33.1% fewer.
  `luaV_execute` and `luaG_traceexec` have exactly the same instruction counts
  in both profiles. Remaining work includes Lua execution, row filling, and
  host API conversion.
- Callback handles are not cached. Both runtimes support cartridges replacing
  callback globals during execution, as the new tests demonstrate. Avoiding
  two lookups per frame does not justify changing those semantics.
- `BDR` callbacks and scanline palette behavior remain active. Callbacks can
  mutate Lua/RAM, and ordinary native runtime users can request the resulting
  framebuffer after a tick. Further gating of palette copies would require an
  explicit rendering-capture policy; this was not needed for the measured wins.
- Requested hashes still fingerprint the declared palette-index/RGBA bytes.
  Hashing packed native VRAM instead would change their meaning. Streaming
  conversion/hashing could be investigated separately with the same byte
  contract. Full-frame recorder work was not changed.

## Observable granularity audit

These are existing declaration differences and possible future bounded
summaries. This pass adds no observable classes.

| Class | PICO-8 today | TIC-80 today | Candidate missing baseline evidence |
|---|---|---|---|
| Lifecycle | Started, halted/crashed | Started/halted on successful execution | Keep error-path coverage distinct from these successful benchmarks |
| Frame progression | Normalized completion and independently selected artifacts | Same | Already present |
| Input activity | Scheduled application and selected typed values | Application/value evidence plus frame-level sampled gamepad mask | PICO-8 has no normalized callback input-read summary; TIC-80 sampling is coarser than individual button reads |
| Graphics | Frame artifacts; cumulative native draw-call counts | Frame artifacts | No normalized bounded graphics-activity summary in either bundle; TIC-80 also lacks a native draw-call summary |
| Audio | Cumulative native audio-API call count | Audio APIs in the current headless subset are stubs with no activity evidence | Explicit bounded API-activity summary if useful; not a claim of faithful audio synthesis |
| Native/debug | Input/frame/runtime-error/execution-summary native kinds | Input/frame/trace/execution-summary kinds; optional normalized trace extension | PICO-8 has no comparable streamed debug-message channel |

Both callback runtimes expose coarser execution detail than CHIP-8's instruction,
control-flow, and state-access observations. A future addition should start with
bounded per-frame counters and explicit prerequisites rather than dense history.

## Reproduce the short checks

Build local provider wheels with `maturin build --release --locked` from each
machine root and install them into an isolated Python 3.12 environment alongside
`glassvm==0.3.0`. The preserved `builds` directories contain the exact baseline
and optimized wheels used here. Then use the existing observation runner:

```sh
python ../glassvm/docs/paper/experiments/run_pilot.py \
  --machines-root "$PWD" --output-dir /absolute/fresh/results \
  --machines pico8 tic80 --frame-limit 600 --repetitions 3 \
  --recipes disabled minimal selected_normalized visual_capability frame_hashes frame_full mixed

python ../glassvm/docs/paper/experiments/run_python_sink_cost.py \
  --machines-root "$PWD" --output-dir /absolute/fresh/null-results \
  --machines pico8 tic80 --frames 1200 --repetitions 3 \
  --modes observation_disabled observed_no_external_sink

python conformance/performance_evidence_probe.py \
  --paper-root ../glassvm/docs/paper --output /absolute/fresh/evidence.json \
  --compare /absolute/baseline-evidence.json
```

Run baseline and optimized environments separately, using the same requests and
fixtures. Do not run profiling, compilation, or other benchmark processes
concurrently with timed trials.

## Files changed

- `pico8/core/src/trace.rs`: scalar counter projection.
- `pico8/core/src/machine/lua_runtime.rs`: counter accessor and runtime tests.
- `pico8/bundle/src/emulator_session.rs`: frame/native/input production guards.
- `tic80/core/src/execution.rs`: native instrumentation selection.
- `tic80/core/src/machine/lua_runtime.rs`: selected events, borrowed trace text,
  empty capture during reconstruction, and clipped packed drawing spans.
- `tic80/core/tests/emulation.rs`: selection, mutation, text, and graphics tests.
- `tic80/bundle/src/emulator_session.rs`: prepared capture mask and payload guards.
- `conformance/performance_evidence_probe.py`: bounded before/after diagnostic.
- `conformance/PERFORMANCE_SANITATION.md`: this report.
