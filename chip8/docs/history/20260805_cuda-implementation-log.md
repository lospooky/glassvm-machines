# Native CUDA implementation log

- **Started:** 2026-08-05
- **Branch:** `codex/chip8-cuda-bundle`
- **Goal:** add exact native CUDA batch execution to the traditional `chip8`
  bundle without PyTorch or `soft_chip8`

## Constraints and decisions

- The user explicitly selected traditional CHIP-8 and excluded PyTorch.
- The implementation extends `machines/chip8`; it does not add a CUDA machine
  ID and does not modify `machines/soft_chip8`.
- `chip8_core::Engine` remains the semantic oracle.
- CUDA C plus a stable Rust Driver API wrapper is the release toolchain.
- The default workspace remains CPU-only and CUDA-toolkit-independent.
- GPU 0 is shared with another Codex task. Material CUDA runs use
  `/tmp/codex-gpu0-night.lock`: announce the estimated window, wait for peer
  acknowledgement, hold file descriptor 9 for the whole window, release it,
  and notify the peer immediately.
- Triton was permitted but not needed. Neither Triton nor PyTorch appears in
  the runtime, artifact-generation path, or dependency graph.

## 2026-08-05 — reconnaissance and specification

Repository audit established that the existing CHIP-8 bundle is already the
canonical three-crate machine implementation and that CUDA belongs under the
core's `src/machine/` extension zone. The development GPU is an NVIDIA GeForce
RTX 3080 Ti with 12 GiB memory and compute capability 8.6. The WSL driver stack
reported Linux driver `610.43.02`, Windows KMD `610.47`, and CUDA UMD 13.3
support. The host had no system `nvcc`, NVRTC, or toolkit installation. This
confirmed the driver-only deployment design: reviewed CUDA C and generated PTX
are checked in; ordinary consumers do not compile kernels at runtime.

The literature survey and normative design were recorded in:

- `docs/research/20260805_chip8-native-cuda-literature-survey.md`
- `docs/architecture/20260805_chip8-native-cuda-backend.md`

The initial acceptance gates are CPU/CUDA full-state parity, deterministic
batch invariance, default CPU-only build health, feature build health, and
actual-device validation under the shared GPU lock.

## 2026-08-05 — kernel, ABI, and host runtime

The implementation added `chip8_cuda_run_v1` as reviewed CUDA C and generated
a checked-in PTX 8.8 artifact targeting virtual architecture `compute_52` with
NVRTC 12.9.86 in a disposable developer environment. The normal runtime never
loads NVRTC. `tools/chip8_cuda/compile_ptx.py` records the source digest,
compiler version, target, and options in the generated header and supports a
byte-for-byte `--check` mode.

One kernel lane owns one VM. The dominant memory and display arrays are
address-major across lanes, while registers, stack, flags, audio, and scalar
state use an explicit 104-byte C-compatible lane record. Reserved fields make
every ABI byte defined. The kernel covers the currently implemented classic,
CHIP-48, Super-CHIP, and XO-CHIP instruction families, six quirk bits, timers,
scripted input and `FX0A`, xorshift64 randomness, two display planes, faults,
frame hashes, and exact counters.

`CudaBatchEvaluator` loads the embedded PTX through the dynamically loaded
CUDA Driver API, validates the device and kernel, reports typed errors, chunks
against checked memory budgets, and preserves caller order. `RunPolicy` frame,
cycle, halt, and stagnation modes are supported; stagnation uses resumable
single-frame launches so it retains the CPU policy's exact host decision.
There is no implicit CPU fallback and unsupported rich event recording is
rejected instead of synthesized.

## 2026-08-05 — GlassVM plugin integration

The `cuda` feature on `chip8_plugin` forwards the matching `chip8_core` feature
and re-exports the typed CUDA API. The current concrete plugin surface is the
explicit `Chip8CudaExt` trait on `Chip8Plugin`, plus equivalent inherent
constructors on `Chip8EmulatorBackend`. This leaves
`MachineBundle::emulator()` and its rich scalar evidence contract CPU-backed;
accelerator use is a visible caller choice.

The CPU oracle also gained a regression fix for scripted `RunPolicy::Cycles`
with `cycles_per_frame == 0`: both the engine and the scripted frame-bound
calculation now use the effective clamped value of one rather than allowing a
division by zero.

## 2026-08-05 — validation evidence

CPU-side and compile-only checks completed before the material GPU window:

| Command | Result |
|---|---|
| `python3 tools/chip8_cuda/compile_ptx.py --check` with NVRTC 12.9.86 | pass; checked-in PTX reproduced byte-for-byte |
| `cargo test -p chip8_core --all-targets` | pass; 132 unit tests plus integration targets |
| `cargo test -p chip8_core --features cuda --lib` | pass; 145 unit tests |
| `cargo test -p chip8_core --features cuda --all-targets --no-run` | pass; CUDA targets compile without a toolkit |
| `cargo clippy -p chip8_core --all-targets --features cuda -- -D warnings` | pass |
| `cargo test -p chip8_plugin --all-targets` | pass |
| `cargo test -p chip8_plugin --features cuda --all-targets --no-run` | pass; plugin extension compiles without initializing a device |

After peer acknowledgement, the actual-device parity suite ran while the
process held file descriptor 9 on `/tmp/codex-gpu0-night.lock`:

```sh
CHIP8_CUDA_TEST=1 CHIP8_CUDA_DEVICE=0 \
  cargo test -p chip8_core --features cuda --lib \
    'machine::cuda::parity_tests::cuda_' -- --nocapture
```

The first integration form passed three opt-in tests. After hardening and
relocation into the canonical core extension zone, the final command passed
**7 tests with 1 intentionally ignored benchmark**. The seven GPU tests cover:

- isolated `8XY*` leaves, `VF` alias edges, carry, borrow, and shift sources;
- isolated `I`, font, BCD, and load/store memory effects;
- each scroll direction in isolation;
- stack boundaries and the repository `smoke.rom` fixture;
- the opcode/quirk/extension/complete-snapshot/fault matrix;
- frame timing, scripted input, RNG, run policies, and stagnation; and
- batch order, lane-local invalid ROMs, repeatability, chunking, and supported
  32/256-thread block invariance.

This is real RTX 3080 Ti execution, not a compile-only or skipped result. A run
without `CHIP8_CUDA_TEST=1` intentionally skips these bodies and must not be
reported as GPU evidence.

### Compute Sanitizer limitation

Two tool generations behaved differently and are recorded separately:

- CUDA-MEMCHECK 11.5.114 successfully instrumented the final seven parity
  tests under `memcheck`, `initcheck`, `racecheck`, and `synccheck`. All four
  runs completed with zero errors; `racecheck` also reported zero hazards.
- Compute Sanitizer 2026.2.1, obtained without a system toolkit installation,
  could not instrument the WSL GPU because the WDDM debugger interface is
  disabled. Its attach limitation occurs before useful kernel instrumentation
  and is **not** reported as a clean run.

The first result supplies actual instrumented-kernel safety evidence. The
second records a newer-tool/platform compatibility limitation without erasing
or overstating the older run.

### Initial unequal-contract performance and runtime profile

The release-mode benchmark used both a mixed four-ROM corpus (repository smoke,
drawing, ALU/RNG, and memory loops) and a homogeneous corpus that repeats the
repository `smoke.rom`. Both run for 60 frames at 12 cycles per frame: 720 guest
cycles per lane. Timings include host packing and host/device transfers; they
exclude evaluator construction and Driver-JIT initialization, which occur
before the per-batch timer. The command was:

```sh
CHIP8_CUDA_TEST=1 CHIP8_CUDA_BENCH=1 \
  cargo test --release -p chip8_core --features cuda --lib \
    cuda_cpu_end_to_end_batch_benchmark -- \
    --ignored --nocapture --test-threads=1
```

The two timed public APIs do not return equal evidence contracts. The CPU call
uses `chip8_core::evaluate_batch`, which also computes interestingness,
trajectory identity, and a flat framebuffer. The CUDA call returns the
deliberately narrower `CudaRunResult`. The tables are therefore public-API
throughput observations; their ratios may overstate the accelerator's advantage
over a hypothetical matched-work CPU path and are not kernel-only speedups.

Mixed-ROM results:

| Batch | CPU elapsed | CUDA elapsed | CPU lanes/s | CUDA lanes/s | API throughput ratio |
|---:|---:|---:|---:|---:|---:|
| 1 | 2,396,084 ns | 25,716,588 ns | 417.348 | 38.885 | 0.093x |
| 32 | 75,360,446 ns | 21,816,830 ns | 424.626 | 1,466.758 | 3.454x |
| 256 | 816,300,650 ns | 138,786,916 ns | 313.610 | 1,844.554 | 5.882x |
| 4096 | 14,182,921,956 ns | 6,041,783,288 ns | 288.798 | 677.946 | 2.347x |

Repeated-`smoke.rom` results:

| Batch | CPU elapsed | CUDA elapsed | CPU lanes/s | CUDA lanes/s | API throughput ratio |
|---:|---:|---:|---:|---:|---:|
| 1 | 3,928,902 ns | 82,766,782 ns | 254.524 | 12.082 | 0.047x |
| 32 | 198,445,688 ns | 110,886,408 ns | 161.253 | 288.584 | 1.790x |
| 256 | 962,446,029 ns | 289,273,103 ns | 265.989 | 884.977 | 3.327x |
| 4096 | 14,421,341,860 ns | 6,129,854,758 ns | 284.024 | 668.205 | 2.353x |

The observed public-API crossover is present by batch 32 for both workloads.
Batch 256 has the largest measured ratio: about 5.88x for the mixed corpus and
3.33x for the repeated ROM. Batch 4096 is about 2.35x in both, while batch 1
favors the CPU API because fixed GPU overhead dominates. These are one
development-host run's unequal-contract observations, not matched-work
speedups or a promise that every ROM, GPU, or concurrent host load has the same
crossover.

The Driver API reported these JIT function properties on the RTX 3080 Ti at
the default 256-thread block size:

| Property | Value |
|---|---:|
| Registers per thread | 80 |
| Local memory per thread | 248 bytes |
| Static shared memory | 0 bytes |
| Kernel maximum block size | 768 threads |
| Active blocks per multiprocessor | 3 |
| Driver-reported PTX version | 52 |
| Driver-reported binary version | 86 |

The embedded textual artifact is PTX ISA 8.8 targeting `compute_52`; the
driver JIT produced `binary_version = 86` for compute capability 8.6. Its final
SHA-256 is
`c869b5d63a65fa6f9e8083948bceba60c5d5532046da3692933a93477c521080`.

## 2026-08-06 — projected-contract refinement and tiled host transpose

The initial tables above compare unequal public result contracts and a single
CPU thread. A refinement pass added a narrower CPU oracle projection that
materializes the CUDA-common execution observables without constructing the
public `RunResult`'s post-run interestingness, trajectory identity, or flat
framebuffer. The `execution_observables_v1` benchmark contract covers:

- complete final snapshot;
- requested/effective seed and ROM hash;
- cycles, completed frames, and termination;
- execution and opcode-class counters;
- optional frame hashes and the always-present final display hash; and
- absent fault evidence for the benchmark's fixed valid, timeout-only corpus.

CUDA backend provenance remains backend-specific and is deliberately outside
the common contract. Its per-lane provenance clone remains inside the CUDA
end-to-end timer. The CPU path remains the real vanilla `Engine`: it still
collects internal events, coverage sets, frame metrics, trajectory SHA state,
frame history, and unique-frame state. The result is therefore an
**observable-contract-aligned implementation-throughput comparison**, not
internally matched work and not a kernel-only speedup.

The harness warms the CUDA evaluator and Rayon global pool, checks every timed
result against an untimed sequential oracle projection, alternates backend
order over five repetitions, and reports the median. The recorded host is an
AMD Ryzen 9 5900X with 24 logical CPUs; Rayon reported 24 worker threads. The
post-refinement command was:

```sh
CHIP8_CUDA_TEST=1 CHIP8_CUDA_BENCH=1 CHIP8_CUDA_DEVICE=0 \
  cargo test --release -p chip8_core --features cuda,parallel --lib \
    cuda_cpu_projected_contract_batch_benchmark -- \
    --ignored --nocapture --test-threads=1
```

The first projected-contract run exposed a host-side large-batch cliff. The
address-major device ABI is correct for adjacent GPU lanes, but the original
lane-at-a-time host pack and reconstruction walked the 64 KiB memory matrix at
`lane_count`-byte strides. At 4096 lanes, adjacent address accesses were one
4 KiB page apart. Packing and reconstruction now transpose memory and display
buffers in 32-address tiles, bounding the active strided pages while preserving
the ABI and every output byte. A 37-lane patterned round-trip test covers tile
boundaries. The CUDA source, checked-in PTX, ABI, and kernel identity did not
change, so the existing instrumented-kernel evidence remains applicable.

The audit also hypothesized that unrecorded frame runs paid a full display hash
after every frame. Source inspection alone suggested that cost, but a guarded
CUDA C rewrite regenerated a byte-identical PTX instruction body with NVRTC
12.9: the compiler already branches over the entire per-frame FNV loop when
`frame_hash_capacity == 0`. The no-op rewrite was discarded, avoiding needless
source/PTX digest churn. Stagnation launches still request one hash per frame,
and the final display hash remains unconditional as required by the result
contract.

Observed CUDA median change after the host-only transpose, under the same
harness in the adjacent device session:

| Workload | Batch | Before | After | Elapsed reduction |
|---|---:|---:|---:|---:|
| Mixed ROMs | 32 | 24,603,771 ns | 21,783,536 ns | 11.5% |
| Mixed ROMs | 256 | 108,066,402 ns | 78,459,642 ns | 27.4% |
| Mixed ROMs | 4096 | 6,272,924,562 ns | 2,266,521,623 ns | 63.9% |
| Repeated `smoke.rom` | 32 | 20,327,644 ns | 19,240,938 ns | 5.3% |
| Repeated `smoke.rom` | 256 | 130,706,347 ns | 111,582,402 ns | 14.6% |
| Repeated `smoke.rom` | 4096 | 5,979,198,210 ns | 2,305,904,587 ns | 61.4% |

This is a same-protocol end-to-end before/after observation, not a phase-level
isolated microbenchmark; it does not assign every changed nanosecond solely to
the transpose or predict the result on another host.

Final mixed-ROM medians:

| Batch | Sequential Engine | Rayon Engine | CUDA Driver | CUDA / sequential speed | CUDA / Rayon speed |
|---:|---:|---:|---:|---:|---:|
| 1 | 2,217,424 ns | 2,294,370 ns | 15,982,459 ns | 0.139x | 0.144x |
| 32 | 68,516,276 ns | 7,479,996 ns | 21,783,536 ns | 3.145x | 0.343x |
| 256 | 538,388,927 ns | 39,936,768 ns | 78,459,642 ns | 6.862x | 0.509x |
| 4096 | 8,884,427,385 ns | 735,598,778 ns | 2,266,521,623 ns | 3.920x | 0.325x |

Final repeated-`smoke.rom` medians:

| Batch | Sequential Engine | Rayon Engine | CUDA Driver | CUDA / sequential speed | CUDA / Rayon speed |
|---:|---:|---:|---:|---:|---:|
| 1 | 2,142,050 ns | 2,125,663 ns | 14,139,365 ns | 0.151x | 0.150x |
| 32 | 68,009,600 ns | 7,314,139 ns | 19,240,938 ns | 3.535x | 0.380x |
| 256 | 546,106,670 ns | 42,328,014 ns | 111,582,402 ns | 4.894x | 0.379x |
| 4096 | 8,791,413,218 ns | 684,147,611 ns | 2,305,904,587 ns | 3.813x | 0.297x |

The speed columns are `CPU elapsed / CUDA elapsed`; values above one mean CUDA
is faster. CUDA crosses the sequential Engine between batch 1 and batch 32 and
reaches 6.86x on the mixed batch at 256. It does **not** overtake the existing
24-thread Rayon path on this host: Rayon remains about 1.97x faster at mixed
batch 256 and 3.08x faster at mixed batch 4096 (2.64x and 3.37x for the repeated
ROM). That is a first-class result, not a hidden caveat: CUDA's demonstrated
advantage is high-throughput execution relative to a single vanilla Engine,
while CPU-parallel evaluation is the stronger choice for this 720-cycle,
full-snapshot workload on the recorded machine.

After the host optimization, the actual-device differential/invariance suite
again passed **7 tests with 1 intentionally ignored benchmark** under the shared
GPU lock. CPU-side `chip8_core --features cuda,parallel --lib` validation
reported 156 passed and one ignored benchmark, both CUDA feature variants were
warning-clean under strict Clippy, and the release benchmark target compiled.

Independent review found no P0 through P2 or merge-blocking issue. It retained one
non-blocking P3 for a future hardening pass: final device-result status is
validated lane by lane after the tiled bulk snapshot reconstruction, so corrupt
device output can cause unnecessary host allocation/transpose work before the
typed `DeviceReported` error. This requires invalid kernel/driver output, does
not affect valid results or the measured hot path, and is not treated as an
adversarial-workspace concern.

### Workspace boundary

The final affected-crate all-target/all-feature command passed: the core
reported 152 passed and one ignored benchmark, and every integration, plugin,
and verifier target passed. Strict Clippy with warnings denied and
package-scoped formatting also passed. Registry conformance passed 27 tests
and registry composition passed 8 tests.

The canonical layout audit reported no CHIP-8 issue. It reached 13 passing
tests and one failing aggregate with exactly two pre-existing Game Boy corpus
findings: the checksum manifest names
`machines/gameboy/docs/reference/corpus/gb-ctr/gbctr.pdf`, which was absent from
the working tree, and exact corpus coverage consequently differed. That
external bundle issue is not CUDA evidence and was not repaired as part of
this change.

A broader CPU-only all-feature workspace sweep compiled every non-registry
package, including the GUI, and then stopped in the unrelated
`chip8_visual_audit` research binary: 91 tests passed and 8 frozen-provenance
tests failed. Source comparison with `master` shows both triggering baseline
conditions are unchanged by this branch: the obsolete nested CHIP-8 verifier
lockfile expected by that tool is absent there too, and the tool's frozen
emulator-version constant already differs from the current CHIP-8 plugin
identity. These failures were recorded rather than folded into the CUDA
change.

## Checkpoints

| Checkpoint | Commit | Verification |
|---|---|---|
| Survey and normative design | `78227f7` | literature, repository layout, execution contract, and acceptance matrix |
| Exact CUDA kernel and PTX | `d93bd96` | NVRTC compile and reproducible checked-in artifact |
| Native Driver API host | `e323b84` | ABI assertions, unit tests, feature build, and strict clippy |
| Scripted zero-rate oracle fix | `cad7bd8` | regression test for clamped cycle/frame conversion |
| Plugin CUDA surface | `be3f5bd` | default plugin tests and feature-gated compile test |
| State-layout documentation correction | `5fc9ec9` | spec and survey aligned with address-major dominant buffers plus 104-byte lane record |
| Actual-device parity matrix | `ea671eb` | three RTX 3080 Ti parity/invariance tests passed initially |
| Canonical generated PTX | `1cfe05b` | artifact normalization and final PTX digest |
| Kernel runtime properties | `6f24fcc` | Driver-JIT register/local/shared/block/occupancy and version identity |
| Canonical bundle assembly | `a7e99cb` | explicit `Chip8CudaExt` without adding state to the canonical bundle struct |
| Fallible host packing | `e66de41` | checked allocation/reservation paths return typed errors instead of panicking |
| Hardened parity and benchmark | `13823c3` | isolated edge cases, canonical test placement, seven actual-device tests, and release benchmark |
| Homogeneous batch benchmark | `4283f72` | same-ROM plus mixed-ROM 1/32/256/4096 release evidence |
| Merge-ready documentation | `35f31d2` | final parity, runtime-profile, sanitizer, benchmark, and checkpoint record |
| Independent review closure | `9f8f8f7` | no P0/P1 findings; qualified unequal API benchmark contracts and representative 32/256 block evidence |
| Observable-contract CPU baselines | `ae6c766` | sequential/Rayon projections, five-sample medians, warm-up, order balancing, and full result checks |
| Tiled host snapshot transpose | `0fcae30` | 37-lane byte-exact round trip, 156 CPU tests, strict Clippy, and repeated 7/7 actual-device parity |
| Refinement evidence closeout | this commit | final projected-contract medians, crossover, Rayon comparison, and merge-ready review |
