//! Structural summary of a Wyrd-16 artifact.

use crate::InspectedArtifact;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactStructure {
    pub byte_len: usize,
    pub rune_count: usize,
    pub has_trailing_byte: bool,
}

impl From<&InspectedArtifact> for ArtifactStructure {
    fn from(artifact: &InspectedArtifact) -> Self {
        Self {
            byte_len: artifact.byte_len,
            rune_count: artifact.runes.len(),
            has_trailing_byte: artifact.trailing_byte.is_some(),
        }
    }
}
