//! Native six-bit tide input.

use crate::CoreError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TideInput(u8);

impl TideInput {
    pub fn from_u64(mask: u64) -> Result<Self, CoreError> {
        if mask > 0x3f {
            return Err(CoreError::new(format!(
                "Hexwell tide mask {mask:#x} exceeds six bits"
            )));
        }
        Ok(Self(mask as u8))
    }

    pub fn mask(self) -> u8 {
        self.0
    }
}
