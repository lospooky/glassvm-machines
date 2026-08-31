//! TIC-80 GlassVM integration.

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
    Tic80NativeEventAdapter, Tic80ObservableAdapter, Tic80StaticAnalyzer, Tic80Verifier,
};
pub use bundle::Tic80Plugin;
pub use emulator_backend::Tic80EmulatorBackend;
pub use tic80_core::{CartChunk, ParsedCart, Tic80Runtime, parse_cart};
