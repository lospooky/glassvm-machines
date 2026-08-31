use glassvm_core::{AnalyzeResult, CapabilityBlob, MachineId, StaticAnalyzerBackend};
use serde_json::json;

use crate::identity::MACHINE_ID;

pub struct Tic80StaticAnalyzer;

impl StaticAnalyzerBackend for Tic80StaticAnalyzer {
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

pub(super) fn analysis_capabilities(
    report: &tic80_verifier::AnalysisReport,
) -> Vec<CapabilityBlob> {
    let chunks = report
        .chunks
        .iter()
        .map(|chunk| json!({"type": chunk.kind, "bank": chunk.bank, "size": chunk.size}))
        .collect::<Vec<_>>();
    vec![CapabilityBlob {
        key: "tic80.cartridge".into(),
        payload: json!({
            "language": report.language,
            "code_bytes": report.code_bytes,
            "chunks": chunks,
            "has_tic_callback": report.has_tic_callback,
            "sha256": report.sha256,
        }),
    }]
}
