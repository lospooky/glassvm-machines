//! PICO-8 GlassVM integration.

mod bundle;
mod contract;
mod descriptor;
mod emulator_backend;
mod emulator_session;
mod identity;
mod normalizer;

pub use bundle::Pico8Plugin;
pub use emulator_backend::Pico8EmulatorBackend;
pub use pico8_core::Pico8Runtime;
