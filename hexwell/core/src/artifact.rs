//! Authoritative immutable catalyst-plate loading.

use crate::{CoreError, WELL_COUNT};

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
}

pub fn validate_artifact(bytes: &[u8]) -> Result<(), CoreError> {
    if bytes.len() != WELL_COUNT {
        return Err(CoreError::new(format!(
            "Hexwell plate must contain exactly {WELL_COUNT} catalyst bytes; found {}",
            bytes.len()
        )));
    }
    Ok(())
}
