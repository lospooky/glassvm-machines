//! Authoritative executable-artifact loading.

use crate::{CoreError, MAX_ROM_BYTES};

/// A validated, aligned Wyrd-16 rune artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact(Vec<u8>);

impl Artifact {
    pub fn parse(bytes: &[u8]) -> Result<Self, CoreError> {
        validate_artifact(bytes)?;
        Ok(Self(bytes.to_vec()))
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn rune_count(&self) -> usize {
        self.0.len() / 2
    }
}

pub fn validate_artifact(bytes: &[u8]) -> Result<(), CoreError> {
    if bytes.len() < 2 {
        return Err(CoreError::new(
            "Wyrd-16 ROM must contain at least one rune (2 bytes)",
        ));
    }
    if bytes.len() > MAX_ROM_BYTES {
        return Err(CoreError::new(format!(
            "Wyrd-16 ROM is {} bytes; maximum is {MAX_ROM_BYTES}",
            bytes.len()
        )));
    }
    if !bytes.len().is_multiple_of(2) {
        return Err(CoreError::new(
            "Wyrd-16 ROM length must be aligned to 2-byte runes",
        ));
    }
    Ok(())
}
