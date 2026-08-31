//! Native analysis and verification reports.

use std::collections::BTreeSet;

use crate::Diagnostic;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisReport {
    pub artifact_format: String,
    pub artifact_version: u8,
    pub source_bytes: usize,
    pub source_lines: usize,
    pub callbacks: Vec<String>,
    pub api_groups: BTreeSet<String>,
    pub sections: Vec<String>,
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
            .any(|diagnostic| diagnostic.severity == crate::Severity::Error)
        {
            "error"
        } else if self.diagnostics.is_empty() {
            "ok"
        } else {
            "warning"
        }
    }
}
