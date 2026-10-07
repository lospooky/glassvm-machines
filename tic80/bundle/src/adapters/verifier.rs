use glassvm_core::{MachineId, VerifierBackend, VerifyResult};
use serde_json::json;

use crate::adapters::analysis_capabilities;
use crate::identity::MACHINE_ID;

pub struct Tic80VerifierBackend;

impl VerifierBackend for Tic80VerifierBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(MACHINE_ID)
    }

    fn verify_bytes(&self, artifact: &[u8]) -> Result<VerifyResult, String> {
        let report = tic80_verifier::verify_bytes(artifact);
        let diagnostics = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                json!({
                    "code": diagnostic.code,
                    "level": diagnostic.severity.as_str(),
                    "message": diagnostic.message,
                })
            })
            .collect();
        let capabilities = report
            .analysis
            .as_ref()
            .map(analysis_capabilities)
            .unwrap_or_default();

        Ok(VerifyResult {
            severity_max: report.severity_max().into(),
            diagnostics,
            capabilities,
        })
    }
}
