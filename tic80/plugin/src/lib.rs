//! TIC-80 GlassVM integration.

mod adapters;
mod bundle;
mod contract;
mod descriptor;
mod emulator_backend;
mod emulator_session;
mod identity;
mod normalizer;

pub use adapters::{Tic80StaticAnalyzerBackend, Tic80VerifierBackend};
pub use bundle::Tic80Plugin;
pub use emulator_backend::Tic80EmulatorBackend;
pub use tic80_core::{ParsedCart, Tic80Runtime, parse_cart};
