//! PICO-8 GlassVM integration.

mod adapters;
mod body;
mod bundle;
mod contract;
mod descriptor;
mod emulator_backend;
mod emulator_session;
mod identity;
mod replay;
mod session_snapshot;
mod trace;

pub use adapters::{
    Pico8NativeEventAdapter, Pico8ObservableAdapter, Pico8StaticAnalyzerBackend,
    Pico8VerifierBackend,
};
pub use bundle::Pico8Plugin;
pub use emulator_backend::Pico8EmulatorBackend;
pub use pico8_core::{Pico8Runtime, RuntimeSnapshot};
pub use replay::Pico8ReplayOutcome;
