use glassvm_core::{MachineId, VerifierBackend, VerifyResult};
use serde_json::json;

use crate::identity::PICO8_ID;

use super::static_analyzer::analysis_capabilities;

pub struct Pico8VerifierBackend;

impl VerifierBackend for Pico8VerifierBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(PICO8_ID)
    }

    fn verify_bytes(&self, artifact: &[u8]) -> Result<VerifyResult, String> {
        let report = pico8_verifier::verify_bytes(artifact);
        let diagnostics = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let mut value = json!({
                    "code": diagnostic.code,
                    "severity": diagnostic.severity.as_str(),
                    "message": diagnostic.message,
                });
                if !diagnostic.apis.is_empty() {
                    value["apis"] = json!(diagnostic.apis);
                }
                value
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
