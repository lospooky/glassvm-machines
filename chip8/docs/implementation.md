# CHIP-8 bundle implementation

The bundle follows the canonical three-crate envelope. `core` owns native
CHIP-8-family execution, `verifier` owns tolerant static reasoning, and
`plugin` adapts those native APIs to GlassVM contracts, sessions, bodies,
snapshots, replay evidence, and normalized traces.

The emulator accepts raw ROM bytes at address `0x200` and supports CHIP-8,
CHIP-48, Super-CHIP, and XO-CHIP configuration profiles. The machine and
emulator identities remain unchanged by this filesystem migration.

Native CUDA acceleration is implemented as an optional, batch-first core
backend. One CUDA thread owns one complete VM lane. The dominant 64 KiB memory
and two display-plane buffers are address-major across lanes; compact scalar
state uses a fixed 104-byte per-lane ABI record. Host packing and final snapshot
reconstruction transpose those dominant buffers in 32-address tiles to bound
cache/TLB pressure without changing the device ABI. The checked-in CUDA C
kernel implements the same instruction, quirk, timer, input, RNG, display, and
fault transitions as `chip8_core::Engine`.

A 37-lane patterned unit test round-trips every memory and display byte through
the tiled layout across tile boundaries. This host-only optimization does not
change the kernel source, checked-in PTX, or their runtime identity.

The release runtime loads checked-in PTX through the dynamically loaded CUDA
Driver API. It does not use PyTorch, Triton, `soft_chip8`, or runtime-generated
machine semantics, and it does not invoke NVRTC. The default build remains
CPU-only and toolkit-independent.

## Cargo features and public API

The `cuda` feature on `chip8_core` enables `CudaBatchEvaluator`, the typed
result/error model, and the dynamic Driver API wrapper. The matching feature on
`chip8_plugin` forwards it, re-exports the CUDA types, and exposes
`Chip8CudaExt` on the concrete bundle:

```rust,ignore
use chip8_plugin::{Chip8CudaExt, Chip8Plugin};
use chip8_plugin::chip8::{EvalConfig, RunPolicy};

let bundle = Chip8Plugin::new();
let evaluator = bundle.cuda_batch_evaluator(0)?;
let config = EvalConfig::new(RunPolicy::Frames(60));
let outcomes = evaluator.evaluate_batch(&roms, &config)?;
```

`Chip8CudaExt::cuda_batch_evaluator_with_options` accepts
`CudaBatchOptions`, including a lane-chunk cap, reserved-memory budget, and
threads-per-block override. `Chip8EmulatorBackend` provides the same two
constructors as inherent methods. The default is 256 threads per block and a
64 MiB reserve inside a 75% free-memory budget.

CUDA selection is explicit. Construction, configuration, allocation, copy,
launch, synchronization, and device validation errors are returned as
`CudaError`; the implementation never silently switches to CPU. Individual
invalid ROMs remain lane-local `Err(String)` values so batch order is stable.
`CudaRunResult` returns the complete final `Snapshot`, termination and fault,
seed/ROM identity, cycles/frames, exact counters, optional frame hashes, and
the resolved device/kernel/launch identity. It intentionally does not invent
the richer scalar session's ordered events, topology, traces, or
interestingness evidence.

## PTX artifact lifecycle

Ordinary users need only a compatible NVIDIA driver. Developers with NVRTC can
regenerate and byte-check the checked-in artifact from the repository root:

```sh
python3 tools/chip8_cuda/compile_ptx.py
python3 tools/chip8_cuda/compile_ptx.py --check
```

Set `NVRTC_LIBRARY=/absolute/path/to/libnvrtc.so` when the library is not on the
dynamic-loader path. The generated PTX header binds its source digest, NVRTC
version, target, and options; a different compiler produces a reviewable
artifact change.

See the
[native CUDA specification](../../../docs/architecture/20260805_chip8-native-cuda-backend.md),
[literature survey](../../../docs/research/20260805_chip8-native-cuda-literature-survey.md),
[testing guide](testing.md), and
[implementation log](history/20260805_cuda-implementation-log.md).
