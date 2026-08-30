//! Native frame timing.

use crate::CoreError;

/// Fixed number of native rune cycles in one visual frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameTiming {
    cycles_per_frame: u32,
}

impl FrameTiming {
    pub fn new(cycles_per_frame: u32) -> Result<Self, CoreError> {
        if cycles_per_frame == 0 {
            return Err(CoreError::new("cycles_per_frame must be non-zero"));
        }
        Ok(Self { cycles_per_frame })
    }

    pub fn cycles_per_frame(self) -> u32 {
        self.cycles_per_frame
    }
}
