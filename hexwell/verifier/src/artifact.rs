//! Tolerant catalyst-plate view.

#[derive(Debug, Clone, Copy)]
pub struct InspectedArtifact<'a> {
    pub bytes: &'a [u8],
}

impl<'a> InspectedArtifact<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }
}
