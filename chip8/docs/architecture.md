# CHIP-8 bundle architecture

The bundle has three one-way layers: `chip8_core` owns deterministic native
execution, `chip8_verifier` depends on core facts and owns non-executing static
reasoning, and `chip8_plugin` adapts both crates to the universal
`MachineBundle` interfaces. Only the plugin depends on GlassVM. Runtime
requests freeze machine quirks, timing, observation policy, inputs, and budgets
before execution.

Snapshots, native events, normalized observations, analyzer output, verifier
output, and replay artifacts are all versioned and machine-bound.

The optional native CUDA batch backend is a hardware implementation inside the
core's `src/machine/cuda/` extension zone. It preserves the `chip8` machine ID
and treats the CPU engine as its semantic oracle; CUDA is not a separate
machine and has no relationship to the differentiable `soft_chip8` runtime.
The `cuda` feature on `chip8_core` owns execution, and the matching feature on
`chip8_plugin` exposes the explicit `Chip8CudaExt` concrete-bundle extension.
The universal scalar
`MachineBundle::emulator()` contract remains CPU-backed so CUDA's narrower
exact batch result is never confused with rich ordered session evidence.
The normative design is
[`docs/architecture/20260805_chip8-native-cuda-backend.md`](../../../docs/architecture/20260805_chip8-native-cuda-backend.md).
