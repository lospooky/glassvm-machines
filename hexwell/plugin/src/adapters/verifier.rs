//! Hexwell: Native verification diagnostic adaptation for GlassVM.

use glassvm_core::{CapabilityId, CapabilityOutput, MachineId, VerifierBackend, VerifyResult};
use serde_json::{Value, json};

use crate::identity::MACHINE_ID;

pub struct HexwellVerifierBackend;

impl VerifierBackend for HexwellVerifierBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(MACHINE_ID)
    }

    fn verify_bytes(&self, rom_bytes: &[u8]) -> Result<VerifyResult, String> {
        let report = hexwell_verifier::verify_bytes(rom_bytes);
        let diagnostics = report
            .diagnostics
            .into_iter()
            .map(|diagnostic| {
                let mut payload = serde_json::Map::from_iter([
                    ("code".into(), json!(diagnostic.code)),
                    ("level".into(), json!(diagnostic.severity.as_str())),
                    ("message".into(), json!(diagnostic.message)),
                ]);
                if let Some(well) = diagnostic.well {
                    payload.insert("well".into(), json!(well));
                }
                if let Some(count) = diagnostic.count {
                    payload.insert("count".into(), json!(count));
                }
                Value::Object(payload)
            })
            .collect();
        Ok(VerifyResult {
            severity_max: report.severity_max.as_str().into(),
            diagnostics,
            capabilities: vec![CapabilityOutput {
                schema: CapabilityId::new("verifier.isa")
                    .expect("verifier capability ID")
                    .schema(),
                value: json!(report.isa),
            }],
        })
    }
}
