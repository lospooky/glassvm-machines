//! Native CHIP-8 ROM artifact validation.

use crate::error::CoreError;
use crate::machine::cpu::{MEM_SIZE, ROM_START};

/// Maximum ROM size accepted by the 64 KiB native engine.
pub const MAX_ROM_BYTES: usize = MEM_SIZE - ROM_START as usize;

/// Validate a raw CHIP-8-family ROM before booting it.
pub fn validate_rom(bytes: &[u8]) -> Result<(), CoreError> {
    if bytes.is_empty() {
        return Err(CoreError::EmptyArtifact);
    }
    if bytes.len() > MAX_ROM_BYTES {
        return Err(CoreError::ArtifactTooLarge {
            actual: bytes.len(),
            maximum: MAX_ROM_BYTES,
        });
    }
    Ok(())
}
