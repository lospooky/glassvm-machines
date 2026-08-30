//! Native analysis and verification reports.

use crate::{ArtifactStructure, Diagnostic, Severity};

#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisReport {
    pub structure: ArtifactStructure,
    pub opcode_counts: [u64; 16],
    pub drawing_runes: u64,
    pub control_flow_runes: u64,
    pub memory_write_runes: u64,
    pub entropy_runes: u64,
    pub input_runes: u64,
    pub reserved_charms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationReport {
    pub severity_max: Severity,
    pub diagnostics: Vec<Diagnostic>,
    pub isa: &'static str,
}
