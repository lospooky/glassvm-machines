//! GlassVM integration for the Hexwell catalyst reactor.

mod adapters;
mod bundle;
mod contract;
mod descriptor;
mod emulator_backend;
mod emulator_session;
mod identity;
mod normalizer;
mod replay;
mod session_snapshot;
mod trace;

pub use adapters::{HexwellStaticAnalyzerBackend, HexwellVerifierBackend};
pub use bundle::HexwellPlugin;
pub use emulator_backend::HexwellEmulatorBackend;
pub use hexwell_core::{
    Catalyst, Family, GRID_HEIGHT, GRID_WIDTH, Materia, WELL_COUNT, neighbor, portal,
    selector_name, well_coordinates, well_index,
};
pub use identity::{MACHINE_ID, MACHINE_VERSION, SEMANTICS};
#[doc(hidden)]
pub use trace::increment_sequence;
