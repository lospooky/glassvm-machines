//! Native Hexwell analysis and verification reports.

use crate::{ArtifactStructure, Diagnostic, Severity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisReport {
    pub structure: ArtifactStructure,
    pub opcode_counts: [u64; 16],
    pub selector_counts: [u64; 8],
    pub ember_polarity: u64,
    pub unlike_neighbor_edges: u64,
    pub neighbor_edges: u64,
    pub boot_well: usize,
    pub boot_byte: u8,
    pub boot_decoded: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationReport {
    pub severity_max: Severity,
    pub diagnostics: Vec<Diagnostic>,
    pub isa: &'static str,
}
