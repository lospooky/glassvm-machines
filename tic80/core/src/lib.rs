//! Native TIC-80 cartridge decoding and deterministic headless execution.
//!
//! The crate owns the binary cartridge, memory map, bounded Lua compatibility
//! runtime, Rust host API, drawing, input, timing, and native snapshots. It has
//! no dependency on GlassVM.

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

pub use artifact::{CartChunk, ParsedCart, parse_cart};
pub use configuration::{
    CYCLES_PER_FRAME, HEIGHT, LUA_INSTRUCTION_BUDGET, LUA_MEMORY_LIMIT_BYTES, MACHINE_ID,
    MAX_CART_BYTES, MAX_CODE_BYTES, RAM_BYTES, SCREEN_BYTES, SEMANTICS, VRAM_BYTES, WIDTH,
    validate_runtime_configuration,
};
pub use emulator::Tic80Runtime;
pub use error::Tic80Result;
pub use event::Tic80Event;
pub use execution::FrameOutcome;
pub use output::Framebuffer;
pub use snapshot::RuntimeSnapshot;
