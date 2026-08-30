//! Tolerant artifact inspection shared by analysis and verification.

use wyrd16_core::Rune;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectedArtifact {
    pub byte_len: usize,
    pub runes: Vec<Rune>,
    pub trailing_byte: Option<u8>,
}

impl InspectedArtifact {
    pub fn inspect(bytes: &[u8]) -> Self {
        let runes = bytes
            .chunks_exact(2)
            .map(|chunk| Rune::decode(chunk[0], chunk[1]))
            .collect();
        Self {
            byte_len: bytes.len(),
            runes,
            trailing_byte: (!bytes.len().is_multiple_of(2)).then(|| bytes[bytes.len() - 1]),
        }
    }
}
