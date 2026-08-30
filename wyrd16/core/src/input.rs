//! Native eight-key input model.

use crate::CoreError;

/// Complete state of Wyrd-16's eight digital keys.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyMask(u8);

impl KeyMask {
    pub fn from_u64(value: u64) -> Result<Self, CoreError> {
        u8::try_from(value)
            .map(Self)
            .map_err(|_| CoreError::new(format!("Wyrd-16 key mask {value:#x} exceeds eight bits")))
    }

    pub fn bits(self) -> u8 {
        self.0
    }
}

impl From<u8> for KeyMask {
    fn from(value: u8) -> Self {
        Self(value)
    }
}
