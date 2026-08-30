//! Native CHIP-8 core errors.

use std::fmt;

/// Errors that can be established before or during native execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    EmptyArtifact,
    ArtifactTooLarge { actual: usize, maximum: usize },
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyArtifact => formatter.write_str("ROM is empty"),
            Self::ArtifactTooLarge { actual, maximum } => {
                write!(formatter, "ROM too large: {actual} bytes (max {maximum})")
            }
        }
    }
}

impl std::error::Error for CoreError {}
