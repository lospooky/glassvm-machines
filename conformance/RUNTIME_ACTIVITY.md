# Selective callback, hook, and RNG activity

Implemented locally on `paper-publication`, 2026-10-21, after the
[graphics activity slice](GRAPHICS_ACTIVITY.md). This remains unreleased source
work; provider metadata still says 0.3.0, but these are not PyPI's 0.3.0 wheels.
The frozen 0.3.0 paper artifacts have not been rewritten or regenerated.

## Decisions and scope

The next observability priority was real computation/callback/RNG activity,
without pretending to trace the Lua instruction stream. This slice exposes two
independently selectable normalized extensions. It adds no reducer, new core
emission channel, whole-run object, recorder format, or Python protocol.

During implementation a separate execution defect was confirmed: TIC-80 looked
up `"math.randomseed"` as a literal global key and therefore did not seed the
actual math-library RNG. Three identical installed-provider runs at seed 7
produced different fingerprints on a tiny RNG-using cart. The user approved:

- fixing initialization in this slice and bumping machine semantics;
- rejecting no-argument reseeding rather than consulting host entropy or
  silently turning it into a reset-to-configuration operation.

These are execution changes, not observation semantics. The two are tested
and described separately below.

## Normalized schemas

Both bundles export `EXECUTION_ACTIVITY` and `RNG_ACTIVITY`. Select them as:

```rust
EventKind::Extension("glassvm.execution_activity".into())
EventKind::Extension("glassvm.rng_activity".into())
```

Or, within an otherwise valid Python observation request:

```python
observation["normalized_events"]["events"] = {
    "kinds": [
        {"extension": "glassvm.execution_activity"},
        {"extension": "glassvm.rng_activity"},
    ]
}
```

Payloads occupy `event.extensions[family_id]` with an explicit `SchemaRef`.

### `glassvm.execution_activity`, schema 1.0.0

```json
{
  "schema": {
    "id": "glassvm.execution_activity",
    "version": { "major": 1, "minor": 0, "patch": 0 }
  },
  "callbacks": { "update": 0, "draw": 0, "main": 1, "border": 144 },
  "instruction_hook": { "ticks": 12, "quantum": 1000 }
}
```

The example is illustrative, not a paper measurement. All counts are unsigned
integers. Callback roles mean successful **host-driver dispatches**:

| Role | PICO-8 | TIC-80 |
|---|---|---|
| `update` | selected `_update` or `_update60` | unused, zero |
| `draw` | `_draw` | unused, zero |
| `main` | unused, zero | combined `TIC` callback |
| `border` | unused, zero | each `BDR(row)` dispatch |

These roles do not assert that TIC-80 separates simulation and drawing like
PICO-8. Counts do not include recursive/manual Lua calls to a callback, all Lua
function calls, or map-remap callbacks. Existing callback lookup/replacement
semantics are preserved; an absent optional callback is not counted.

`ticks` is the number of successful decrements by the **existing instruction
budget hook** during the frame. `quantum` is its configured interval, currently
1,000 in both runtimes. The hook's VM phase can cross frame boundaries and its
budget is reset by existing frame execution. It is coarse quota accounting,
not an exact per-frame instruction count, CPU-cycle count, wall-clock time,
source-statement count, or CPU-independent cost metric. No new hook is added,
and its frequency/budget behavior is not changed for observation.

Emit at most one summary after a successful frame with at least one counted
callback or hook tick. An idle PICO-8 frame with neither emits nothing.

### `glassvm.rng_activity`, schema 1.0.0

```json
{
  "schema": {
    "id": "glassvm.rng_activity",
    "version": { "major": 1, "minor": 0, "patch": 0 }
  },
  "random_calls": 3,
  "reseed_calls": 1
}
```

This records successful calls reaching the installed compatibility APIs:
`rnd`/`srand` in PICO-8, `math.random`/`math.randomseed` in TIC-80. It does not
count internal PRNG transitions or rejections inside an API call. Cached
aliases of those functions remain observable. A cartridge replacement function
is not magically interpreted as a native RNG operation.

No argument, generated value, seed, or RNG state is emitted. This is not an
entropy or randomness-quality measurement. With no covered call there is no
event. Initialization, top-level code, `_init`, `BOOT`, reconstruction, and
failed/incomplete frames do not produce completed-frame RNG summaries.

### Independence and coordinates

Each family is selectable independently of the other, graphics activity,
`FrameCompleted`, frame artifacts, and native evidence. `All` includes them;
exact `Kinds` requests may omit them. Existing broad selector semantics are
unchanged. No existing visual capability acquires new prerequisites.

`frame` identifies the completed zero-based semantic frame; `step` uses the
bundle's existing frame-step convention. Sequence values retain their existing
allocation and are not a global semantic merge key. Budgets account for both
families as ordinary normalized events. Incompleteness remains evidence
accounting, not a machine execution failure.

## Bounded production

Selection flags resolve from the prepared request at session construction.
Callback counts are fixed-size frame-local scalars. Hook ticks come from a
scalar read of existing budget accounting, only when selected.

Each native runtime owns two RNG counters and an enable flag, outside snapshots.
The capture scope resets/enables them only when requested and disables them on
all exits, including errors. API calls check the flag; unselected calls do not
accumulate evidence. Per-frame successful call counts are bounded by the Lua
instruction quota. No arguments, calls, callbacks, frames, or emissions are
retained as a history. Native counters are separate machine-owned types; no
GlassVM dependency is introduced into either native core.

TIC-80 installs thin Rust wrappers around the original math functions before
cartridge loading, preserving cached aliases, Lua arguments/results, and error
propagation. The wrapper/gate has a cost even without capture; this is not a
claim of literally zero instrumentation overhead. There is no worker or queue.

## TIC-80 seeded execution correction

Machine identity changes from `tic80-lua-semantics.v2` to
`tic80-lua-semantics.v3`. The configuration schema/field shape is unchanged:
this repairs the promised ownership of the existing `machine_seed`, rather
than adding another seed knob. The corrected semantics seed the actual
`math.randomseed` function **before any cartridge code**, with explicit Lua
seed words `(machine_seed as i64, 0)`. The GlassVM configuration accepts the
existing nonnegative signed-integer range; the native constructor's u64 values
use a bit-preserving signed interpretation for Lua.

Loading and `BOOT` can then consume RNG deterministically or explicitly reseed
it. Cartridge numeric reseeds override the current RNG state at that point;
their existing Lua return values and supported argument forms are preserved.
`math.randomseed()` and `math.randomseed(nil)` fail explicitly, including during
loading/BOOT. No host-entropy initialization fallback remains. Calls that
never reach the installed implementation are not subject to its API policy.

Prepared execution identities now bind the changed machine semantics. Old
TIC-80 provenance and snapshots bound to the old prepared request are not
interchangeable with new sessions. Snapshot layout is unchanged; observation
counters are not added to it. Deterministic reconstruction now works for the
RNG-using continuation fixtures. No cross-Lua-version/host bit-identity or full
upstream native fidelity is claimed.

## Verification and spot measurements

All PICO-8/TIC-80 core and bundle suites, machine conformance, formatting,
and Clippy with denied warnings pass. New checks cover:

- exact selectors, versioned payloads, positive-only RNG events, role counts;
- real hook ticks on finite compute-heavy carts;
- observation-on/off equality of machine state and frame content;
- initialization exclusion, cached RNG aliases, replaced callbacks;
- cleanup after failures, continuation without retained activity, hard budgets;
- same/different seeds, load/BOOT/frame randomness, numeric reseed arguments and
  results, rejected entropy reseeding, RNG-using continuation.

An installed-wheel regression also confirms three seed-7 runs have identical
frame fingerprints and seed 8 differs for the tiny regression cart.

Artifacts are under:
`/home/spooky/workspace/glassvm-paper-results/execution-observation-20261021/`.
Both `before/` and `after/` contain only the two-machine, three-recipe,
three-repetition 600-frame spot matrix: 18 trials each. All 36 trials completed
execution, evidence, recorder finalization, and publication. The full paper
suite was **not** rerun. `before` already includes the earlier graphics slice;
`after` adds this slice and the approved TIC-80 seed correction. Exact wheels
are preserved under `builds/{before,after}`.

| Machine | All normalized before → after | Existing selected count | Selected reduction within new build |
|---|---:|---:|---:|
| PICO-8 | 1,202 → 2,402 | 600 | 75.02% |
| TIC-80 | 1,802 → 2,402 | 1,200 | 50.04% |

Over 600 frames the PICO-8 workload reports 600 update + 600 draw dispatches,
25,482 hook ticks, and 672,000 RNG calls summarized in 600 RNG events. The TIC-80
workload reports 600 main dispatches, 4,600 hook ticks, no border dispatches,
and no RNG events. Controlled conformance fixtures exercise its RNG and border
paths; paper-cart inactivity is not filled with fake events.

`projections.json` contains eight checks through simultaneous Python consumers
and the Rust recorder: selected, execution-only, RNG-only, all-normalized for
each bundle. Each family has the same frame/step/payload projection when
requested alone or with all normalized events. No native/frame channel was
requested. Evidence, Python delivery, recorder, and publication outcomes all
passed independently.

Median execution + finalization/publication, milliseconds:

| Machine | Request | Before | After |
|---|---|---:|---:|
| PICO-8 | Disabled | 391.637 | 394.964 |
| PICO-8 | All normalized | 432.935 | 477.585 |
| PICO-8 | Selected normalized | 412.020 | 417.067 |
| TIC-80 | Disabled | 143.161 | 147.373 |
| TIC-80 | All normalized | 208.372 | 232.541 |
| TIC-80 | Selected normalized | 181.384 | 183.212 |

These are descriptive short local medians, not significance or cross-machine
performance claims. Broader observation has a real cost; fewer emitted records
do not imply proportional reduction of underlying machine work.

The 20-case short evidence probe also completed. All eight narrow PICO-8 cases
match the preceding graphics build exactly, including receipts and recorder
accounting. TIC-80 raw fingerprints intentionally differ with its changed
semantics/preparation provenance; do not erase those identities to manufacture
byte-equivalence. Its narrow counts, accounting, and outcome objects remain
unchanged for this paper cart. Capture-on/off semantic equality is tested
within the new semantics separately. `All` cases include new evidence by design.

### Reproduce

Install the explicit locally built wheels into a dedicated environment, then
from the machines checkout run:

```sh
python conformance/runtime_activity_probe.py \
  --paper-root ../glassvm/docs/paper --output /chosen/result-dir/projections.json
```

The limited timing matrix uses the unchanged GlassVM paper pilot:

```sh
python docs/paper/experiments/run_pilot.py \
  --machines-root ../glassvm-machines --output-dir /chosen/result-dir/matrix \
  --machines pico8 tic80 --recipes disabled all_normalized selected_normalized \
  --frame-limit 600 --repetitions 3
```

Run that command from the GlassVM checkout. Source hashes and installed wheel
URLs/hashes are recorded by the projection probe; distribution version alone
does not identify these local builds.

## Next priorities

The raw bounded activity families are implemented. Capability granularity is
next: choose useful derived outputs and exact prerequisites, rather than
broadening visual requirements or declaring raw evidence itself a capability.
Then address meaningful input consumption and selective state-access evidence.
These remain open; no Lua-bytecode tracing, new reducer, or full regeneration
is included here. New release coordinates/pins and affected paper regeneration
remain mandatory before publication claims use this code.
