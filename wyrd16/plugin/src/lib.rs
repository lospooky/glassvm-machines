//! GlassVM integration for the Wyrd-16 rune computer.

mod adapters;
mod bundle;
mod contract;
mod descriptor;
mod emulator_backend;
mod emulator_session;
mod identity;
mod normalizer;
mod session_snapshot;
mod trace;

pub use adapters::{Wyrd16StaticAnalyzerBackend, Wyrd16VerifierBackend};
pub use bundle::Wyrd16Plugin;
pub use emulator_backend::Wyrd16EmulatorBackend;
pub use identity::MACHINE_ID;
pub use wyrd16_core::{
    DISPLAY_HEIGHT, DISPLAY_PIXELS, DISPLAY_WIDTH, MAX_ROM_BYTES, MEMORY_BYTES, Rune,
};
