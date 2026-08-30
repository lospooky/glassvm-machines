//! Native machine configuration.

pub const MACHINE_ID: &str = "wyrd16";
pub const SEMANTICS: &str = "wyrd16-semantics.v1";
pub const MACHINE_VERSION: &str = SEMANTICS;
pub const MEMORY_BYTES: usize = 4096;
pub const DISPLAY_WIDTH: usize = 64;
pub const DISPLAY_HEIGHT: usize = 64;
pub const DISPLAY_PIXELS: usize = DISPLAY_WIDTH * DISPLAY_HEIGHT;
pub const MAX_ROM_BYTES: usize = MEMORY_BYTES;

/// Configuration required to boot the deterministic native machine.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MachineConfiguration {
    pub seed: u64,
}
