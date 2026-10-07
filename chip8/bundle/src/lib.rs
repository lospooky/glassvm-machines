//! GlassVM integration for the CHIP-8 machine family.

mod adapters;
mod bundle;
mod contract;
mod descriptor;
mod emulator_backend;
mod emulator_session;
mod identity;
mod live_input;
mod normalizer;
mod session_snapshot;
mod trace;

pub use adapters::{Chip8StaticAnalyzerBackend, Chip8VerifierBackend};
pub use bundle::Chip8Plugin;
#[cfg(feature = "cuda")]
pub use emulator_backend::Chip8CudaExt;
pub use emulator_backend::Chip8EmulatorBackend;
pub use identity::{MACHINE_ID, SEMANTICS};

/// Native CUDA batch types exposed only when the bundle's explicit `cuda`
/// feature is selected. These results intentionally do not implement or imply
/// the richer scalar-session trace/evidence contract of [`Chip8EmulatorBackend`].
#[cfg(feature = "cuda")]
pub use chip8_core::{
    CudaBackendIdentity, CudaBatchEvaluator, CudaBatchOptions, CudaDeviceIdentity, CudaError,
    CudaExecutionCounters, CudaFault, CudaKernelIdentity, CudaKernelRuntimeProperties,
    CudaLaunchIdentity, CudaRunResult,
};

#[doc(hidden)]
pub use emulator_backend::resolve_quirks;
#[doc(hidden)]
pub use emulator_session::{Chip8Session, Chip8SessionSnapshot, Chip8SessionSnapshotPayload};
#[doc(hidden)]
pub use identity::{CHIP8_MAX_ROM_BYTES, DISPLAY_FRAME_PAYLOAD_SCHEMA_VERSION, schema};
#[doc(hidden)]
pub use session_snapshot::{
    SNAP_BUF, SNAP_CODEC_VERSION, SNAP_HEADER_LEN, SNAP_MAGIC, SNAP_MEM, SNAP_PAYLOAD_LEN,
    snapshot_from_bytes, snapshot_to_bytes,
};
#[doc(hidden)]
pub use trace::{key_mask_location, key_wait_location, stack_pointer_location};
