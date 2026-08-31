use glassvm_core::{AnalyzeResult, CapabilityBlob, MachineId, StaticAnalyzerBackend};
use serde_json::json;

use crate::identity::PICO8_ID;

pub struct Pico8StaticAnalyzerBackend;

impl StaticAnalyzerBackend for Pico8StaticAnalyzerBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(PICO8_ID)
    }

    fn analyze_bytes(&self, artifact: &[u8]) -> Result<AnalyzeResult, String> {
        let report = pico8_verifier::analyze_bytes(artifact)?;
        Ok(AnalyzeResult {
            capabilities: analysis_capabilities(&report),
        })
    }
}

pub(super) fn analysis_capabilities(
    report: &pico8_verifier::AnalysisReport,
) -> Vec<CapabilityBlob> {
    vec![CapabilityBlob {
        key: "pico8.static_summary".into(),
        payload: json!({
            "artifact_format": report.artifact_format,
            "artifact_version": report.artifact_version,
            "source_bytes": report.source_bytes,
            "source_lines": report.source_lines,
            "callbacks": report.callbacks,
            "api_groups": report.api_groups,
            "sections": report.sections,
        }),
    }]
}
