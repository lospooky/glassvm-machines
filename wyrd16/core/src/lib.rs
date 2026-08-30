//! Deterministic native execution semantics for the Wyrd-16 rune computer.

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

pub use artifact::{Artifact, validate_artifact};
pub use configuration::{
    DISPLAY_HEIGHT, DISPLAY_PIXELS, DISPLAY_WIDTH, MACHINE_ID, MACHINE_VERSION, MAX_ROM_BYTES,
    MEMORY_BYTES, MachineConfiguration, SEMANTICS,
};
pub use emulator::Emulator;
pub use error::CoreError;
pub use event::{NativeEffectKind, StepEffect};
pub use execution::execute_step;
pub use input::KeyMask;
pub use machine::instruction::Rune;
pub use output::CanvasView;
pub use snapshot::NativeSnapshot;
pub use state::MachineState;
pub use timing::FrameTiming;
pub use trace::NativeStepTrace;
