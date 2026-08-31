//! Native PICO-8 cartridge decoding and deterministic headless execution.
//!
//! The crate owns cartridge formats, machine memory, the compatibility Lua
//! runtime, controller input, graphics output, timing, and native snapshots.
//! It has no dependency on GlassVM.

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

pub use artifact::{Cartridge, CartridgeFormat};
pub use configuration::{MACHINE_ID, SEMANTICS};
pub use emulator::Pico8Runtime;
pub use error::CartridgeError;
pub use machine::cartridge::{
    CART_DATA_BYTES, DISPLAY_HEIGHT, DISPLAY_WIDTH, MAX_SOURCE_CHARACTERS, RAM_BYTES, SCREEN_BYTES,
    SCREEN_START,
};
pub use output::{get_pixel, set_pixel, unpack_framebuffer};
pub use snapshot::RuntimeSnapshot;
