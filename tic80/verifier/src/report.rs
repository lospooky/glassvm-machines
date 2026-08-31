//! Native analysis and verification reports.

use tic80_core::CartChunk;

use crate::{Diagnostic, Severity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisReport {
    pub language: String,
    pub code_bytes: usize,
    pub chunks: Vec<CartChunk>,
    pub has_tic_callback: bool,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationReport {
    pub analysis: Option<AnalysisReport>,
    pub diagnostics: Vec<Diagnostic>,
}

impl VerificationReport {
    pub fn severity_max(&self) -> &'static str {
        if self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
        {
            "error"
        } else if self.diagnostics.is_empty() {
            "ok"
        } else {
            "warning"
        }
    }
}
