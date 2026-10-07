# CHIP-8 machine bundle

The GlassVM bundle crate is in `bundle/`. It connects the CHIP-8, CHIP-48,
Super-CHIP, and XO-CHIP implementations to GlassVM's machine descriptor,
artifact/input contract, emulator session, evidence channels, and normalizers.

The native deterministic emulator lives in `core/`; non-executing artifact
analysis and verification live in `verifier/`; GlassVM integration lives in
`bundle/`. See `docs/architecture.md`, `docs/implementation.md`, and the
canonical on-disk lifecycle test in `bundle/tests/bundle.rs`.

## Native CUDA batch execution

The traditional CHIP-8 bundle also has an optional exact, native CUDA batch
backend. It executes the same CHIP-8/CHIP-48/Super-CHIP/XO-CHIP semantics as
`chip8_core::Engine`; it is not the differentiable `soft_chip8` machine and it
uses neither PyTorch nor Triton. The normal runtime dynamically loads the CUDA
Driver API and the checked-in PTX, so users need a compatible NVIDIA driver but
not a CUDA toolkit, `nvcc`, or NVRTC.

Enable the API at the layer you consume:

```sh
cargo build -p chip8_core --features cuda
cargo build -p chip8_bundle --features cuda
```

The core exposes `CudaBatchEvaluator`. The bundle forwards it through the
explicit `Chip8CudaExt` extension on `Chip8Plugin` and through inherent methods
on `Chip8EmulatorBackend`:

```rust,ignore
use chip8_bundle::{Chip8CudaExt, Chip8Plugin};
use chip8_bundle::chip8::{EvalConfig, RunPolicy};

let bundle = Chip8Plugin::new();
let evaluator = bundle.cuda_batch_evaluator(0)?;
let config = EvalConfig::new(RunPolicy::Frames(60));
let outcomes = evaluator.evaluate_batch(&roms, &config)?;
```

CUDA selection is explicit and never silently falls back to CPU. The scalar
GlassVM emulator continues to provide the richer trace/evidence contract;
`CudaRunResult` provides exact final machine state, termination, hashes, and
the counters the kernel can produce without fabrication.

On the recorded RTX 3080 Ti release workloads, CUDA overtook the sequential
vanilla `Engine` projection by batch 32. At batch 256 it was 6.86x faster for
the mixed-ROM corpus and 4.89x for repeated `smoke.rom`; at batch 4096 the
ratios were 3.92x and 3.81x. A 24-thread Rayon projection remained faster than
CUDA at every measured batch, by roughly 2.0x to 3.4x at batches 256 and 4096.
These are five-sample median, end-to-end implementation-throughput results for
the same materialized execution observables. The vanilla CPU still performs
its internal rich telemetry work, so they are neither internally matched-work
nor kernel-only speedups and are not promises for every host or ROM. The
implementation log records the exact contract, timings, and runtime properties.

Start with the [implementation guide](docs/implementation.md),
[testing guide](docs/testing.md),
[normative backend specification](../../docs/architecture/20260805_chip8-native-cuda-backend.md),
[literature survey](../../docs/research/20260805_chip8-native-cuda-literature-survey.md),
and [dated implementation log](docs/history/20260805_cuda-implementation-log.md).
