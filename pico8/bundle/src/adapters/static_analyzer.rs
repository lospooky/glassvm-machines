use glassvm_core::{
    AnalyzeResult, CapabilityId, CapabilityOutput, MachineId, StaticAnalyzerBackend,
};
use serde_json::json;

use crate::identity::MACHINE_ID;

pub struct Pico8StaticAnalyzerBackend;

impl StaticAnalyzerBackend for Pico8StaticAnalyzerBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(MACHINE_ID)
    }

    fn analyze_bytes(&self, artifact: &[u8]) -> Result<AnalyzeResult, String> {
        let report = pico8_verifier::analyze_bytes(artifact)?;
        Ok(AnalyzeResult {
            capabilities: analysis_capabilities(&report),
        })
    }
}

pub(crate) fn analysis_capabilities(
    report: &pico8_verifier::AnalysisReport,
) -> Vec<CapabilityOutput> {
    vec![capability(
        "pico8.static_profile",
        json!({
            "artifact_format": report.artifact_format,
            "artifact_version": report.artifact_version,
            "source_bytes": report.source_bytes,
            "source_lines": report.source_lines,
            "callbacks": report.callbacks,
            "api_groups": report.api_groups,
            "sections": report.sections,
        }),
    )]
}

fn capability(id: &str, value: serde_json::Value) -> CapabilityOutput {
    let id = CapabilityId::new(id).expect("static analyzer capability ID");
    CapabilityOutput {
        schema: id.schema(),
        value,
    }
}
