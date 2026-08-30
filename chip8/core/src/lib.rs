//! Deterministic native CHIP-8, CHIP-48, Super-CHIP, and XO-CHIP emulator.
//!
//! This crate owns machine semantics and native evidence only. GlassVM
//! orchestration and static acceptance policy live in sibling crates.

pub mod artifact;
pub mod configuration;
pub mod emulator;
pub mod error;
pub mod event;
pub mod execution;
pub mod input;
pub mod machine;
pub mod output;
pub mod snapshot;
pub mod state;
pub mod timing;
pub mod trace;

pub use configuration::{MACHINE_ID, QuirksConfig, SEMANTICS};
pub use emulator::{Engine, FrameResult};
pub use event::{Event, EventLog, Timer};
#[cfg(feature = "parallel")]
pub use execution::evaluate_batch_parallel;
pub use execution::{EvalConfig, FrameRecord, RunResult, effective_seed, evaluate, evaluate_batch};
pub use input::{InputEvent, InputScript, KeyAction};
pub use machine::coverage::{CoverageSummary, CoverageTracker};
pub use machine::cpu::{Cpu, KeyWait, ROM_START, StepResult};
#[cfg(feature = "cuda")]
pub use machine::cuda::{
    CudaBackendIdentity, CudaBatchEvaluator, CudaBatchOptions, CudaDeviceIdentity, CudaError,
    CudaExecutionCounters, CudaFault, CudaKernelIdentity, CudaKernelRuntimeProperties,
    CudaLaunchIdentity, CudaRunResult,
};
pub use machine::display::Display;
pub use machine::keypad::Keypad;
pub use machine::metrics::{
    InterestingnessSummary, TRAJECTORY_IDENTITY_DEFINITION, TrajectoryIdentity,
};
pub use machine::policy::RunPolicy;
pub use machine::randomness::Rng;
pub use machine::replay::{Replay, ReplayRecorder};
pub use machine::summary::{ExecutionSummary, TerminationReason};
pub use snapshot::Snapshot;
pub use trace::{
    CompactState, FrameTimerTraceRecord, MemoryChange, MemoryRead, StepTraceRecord, TimerState,
};

pub use machine::coverage;
pub use machine::cpu;
#[cfg(feature = "cuda")]
pub use machine::cuda;
pub use machine::display;
pub use machine::font;
pub use machine::keypad;
pub use machine::metrics;
pub use machine::policy;
pub use machine::randomness;
pub use machine::replay;
pub use machine::summary;
