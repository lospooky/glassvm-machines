use glassvm_core::{
    AnalyzeResult, CapabilityId, CapabilityOutput, MachineId, StaticAnalyzerBackend,
};
use serde_json::json;

use crate::identity::MACHINE_ID;

pub struct Tic80StaticAnalyzerBackend;

impl StaticAnalyzerBackend for Tic80StaticAnalyzerBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(MACHINE_ID)
    }

    fn analyze_bytes(&self, artifact: &[u8]) -> Result<AnalyzeResult, String> {
        let report = tic80_verifier::analyze_bytes(artifact)?;
        Ok(AnalyzeResult {
            capabilities: analysis_capabilities(&report),
        })
    }
}

pub(crate) fn analysis_capabilities(
    report: &tic80_verifier::AnalysisReport,
) -> Vec<CapabilityOutput> {
    vec![capability(
        "tic80.static_profile",
        json!({
            "language": report.language,
            "code_bytes": report.code_bytes,
            "chunks": report.chunks,
            "has_tic_callback": report.has_tic_callback,
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
