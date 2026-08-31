use glassvm_core::{MachineId, VerifierBackend, VerifyResult};
use serde_json::{Map, Value, json};

use crate::adapters::analysis_capabilities;
use crate::identity::MACHINE_ID;

pub struct Pico8VerifierBackend;

impl VerifierBackend for Pico8VerifierBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(MACHINE_ID)
    }

    fn verify_bytes(&self, artifact: &[u8]) -> Result<VerifyResult, String> {
        let report = pico8_verifier::verify_bytes(artifact);
        let diagnostics = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let mut payload = Map::from_iter([
                    ("code".into(), json!(diagnostic.code)),
                    ("level".into(), json!(diagnostic.severity.as_str())),
                    ("message".into(), json!(diagnostic.message)),
                ]);
                if !diagnostic.apis.is_empty() {
                    payload.insert("apis".into(), json!(diagnostic.apis));
                }
                Value::Object(payload)
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
