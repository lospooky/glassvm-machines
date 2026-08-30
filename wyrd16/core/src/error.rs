//! Native core errors.

use std::{error::Error, fmt};

/// Failure to load or execute a Wyrd-16 artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreError(String);

impl CoreError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for CoreError {}
