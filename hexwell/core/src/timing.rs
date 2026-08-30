//! Native sweep timing.

use crate::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameTiming(u32);

impl FrameTiming {
    pub fn new(sweeps_per_frame: u32) -> Result<Self, CoreError> {
        if sweeps_per_frame == 0 {
            return Err(CoreError::new("cycles_per_frame must be non-zero"));
        }
        Ok(Self(sweeps_per_frame))
    }

    pub fn sweeps_per_frame(self) -> u32 {
        self.0
    }
}
