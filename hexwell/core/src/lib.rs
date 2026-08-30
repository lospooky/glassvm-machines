//! Deterministic native reactor semantics for Hexwell.

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
pub use configuration::{MACHINE_ID, MACHINE_VERSION, MachineConfiguration, SEMANTICS};
pub use emulator::Emulator;
pub use error::CoreError;
pub use event::{CellChange, FeedOutcome, FiringRecord, SweepOutcome, TransferRecord};
pub use execution::execute_sweep;
pub use input::TideInput;
pub use machine::instruction::{
    CENTER_WELL, CENTER_X, CENTER_Y, Catalyst, Family, GRID_HEIGHT, GRID_WIDTH, Materia,
    WELL_COUNT, neighbor, opposite, portal, selector_name, well_coordinates, well_index,
};
pub use machine::runtime::{ReactorState, Telemetry, WellState, connected_matter_components};
pub use output::ReactorOutput;
pub use snapshot::NativeSnapshot;
pub use state::NativeState;
pub use timing::FrameTiming;
pub use trace::NativeSweepTrace;
