# CHIP-8 bundle testing

Core tests characterize artifact loading, deterministic execution, native
snapshot continuation, and input-driven emulation. Verifier tests exercise
analysis and acceptance diagnostics without executing the artifact. Plugin
tests exercise the public bundle, body, emulator session, observability, trace,
snapshot, reset, and replay paths against `fixtures/smoke.rom`.

Run all three layers with:

```sh
cargo test -p chip8_core -p chip8_verifier -p chip8_plugin --all-targets --all-features
```

The CUDA feature has compile-only and actual-device tiers. Compile-only gates
do not need an NVIDIA device or CUDA toolkit because the kernel artifact is
precompiled and the Driver API is loaded dynamically:

```sh
cargo test -p chip8_core --features cuda --all-targets --no-run
cargo test -p chip8_plugin --features cuda --all-targets --no-run
cargo clippy -p chip8_core -p chip8_plugin --all-targets --features cuda -- -D warnings
```

Actual-device differential and invariance tests are opt-in so CPU-only CI does
not report false failures. They compare complete final snapshots and frame
hashes, termination, counters, timing, inputs, randomness, faults, and batch
invariance against `chip8_core::Engine`:

```sh
CHIP8_CUDA_TEST=1 CHIP8_CUDA_DEVICE=0 \
  cargo test -p chip8_core --features cuda --lib \
    'machine::cuda::parity_tests::cuda_' -- --nocapture
```

`CHIP8_CUDA_DEVICE` defaults to `0`. A passing command is actual GPU evidence;
the same test without `CHIP8_CUDA_TEST=1` reports an intentional skip and is
not parity evidence.

## Shared GPU lock

Before material work on a shared device, announce the expected window and wait
for acknowledgement. Hold file descriptor 9 on the common lock for the entire
window, including sanitizer/profile subprocesses:

```sh
exec 9>/tmp/codex-gpu0-night.lock
flock 9

CHIP8_CUDA_TEST=1 CHIP8_CUDA_DEVICE=0 \
  cargo test -p chip8_core --features cuda --lib \
    'machine::cuda::parity_tests::cuda_' -- --nocapture

flock -u 9
```

Notify peers immediately after release. The Rust test mutex only serializes
tests inside one test process; it does not replace the cross-task file lock.

## Device instrumentation

On a runner where CUDA debugging is supported, exercise the opt-in suite under
`memcheck`, `initcheck`, `racecheck`, and `synccheck`, while holding the same
lock. An attach or instrumentation failure is not a clean sanitizer result.
The 2026-08-05 session obtained two distinct results: CUDA-MEMCHECK 11.5.114
successfully instrumented the final seven parity tests and reported zero
errors in all four modes (and zero race hazards), while Compute Sanitizer
2026.2.1 could not attach under WSL because the GPU's WDDM debugger interface
was disabled. The older clean run is valid evidence for the instrumented
kernel; the newer tool's failure is separately recorded as a platform
limitation, not another pass.

## Performance benchmark

The ignored release-mode benchmark measures CPU and CUDA end to end, including
host packing and transfers, for both mixed-ROM and repeated-`smoke.rom`
batches of 1, 32, 256, and 4096:

```sh
CHIP8_CUDA_TEST=1 CHIP8_CUDA_BENCH=1 CHIP8_CUDA_DEVICE=0 \
  cargo test --release -p chip8_core --features cuda,parallel --lib \
    cuda_cpu_projected_contract_batch_benchmark -- \
    --ignored --nocapture --test-threads=1
```

It must run inside the same GPU lease. The benchmark warms the CUDA evaluator
and Rayon pool, alternates backend order across five samples, and reports the
median for sequential `Engine`, Rayon `Engine` (24 threads on the recorded
host), and CUDA Driver API execution. Every timed output is checked against an
untimed oracle projection.

CUDA timing includes per-call planning and lane preparation, tiled host
packing/unpacking, allocation, host/device copies, launch, synchronization,
and result construction. It excludes evaluator construction/Driver JIT,
corpus and untimed-reference construction, warm-up, and assertions performed
after each timer stops. CPU timings likewise stop before equivalence checks.

The `execution_observables_v1` projection aligns complete snapshot, seed/ROM
identity, cycles, frames, termination, counters, optional frame hashes, final
display hash, and the valid fixed corpus's absent fault. Backend provenance is
intentionally backend-specific and outside that common contract.

The CPU projections avoid constructing the public API's interestingness,
trajectory identity, and flat framebuffer, but the vanilla `Engine` still
performs its internal event, coverage, frame-metric, and trajectory telemetry.
CUDA also retains per-lane backend provenance construction inside its timer.
Treat the ratios as observable-contract-aligned implementation throughput, not
internally matched work or kernel-only speedup. Current device identity,
runtime properties, medians, and crossover are recorded in the
[dated implementation log](history/20260805_cuda-implementation-log.md).
