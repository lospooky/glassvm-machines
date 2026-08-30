//! Conversion from native diagnostics into the GlassVM verifier report.

use glassvm_core::{CapabilityId, CapabilityOutput, MachineId, VerifierBackend, VerifyResult};
use serde_json::{Map, Value, json};

use crate::identity::MACHINE_ID;

pub struct Wyrd16VerifierBackend;

impl VerifierBackend for Wyrd16VerifierBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(MACHINE_ID)
    }

    fn verify_bytes(&self, rom_bytes: &[u8]) -> Result<VerifyResult, String> {
        let report = wyrd16_verifier::verify_bytes(rom_bytes);
        let diagnostics = report
            .diagnostics
            .into_iter()
            .map(|diagnostic| {
                let mut object = Map::from_iter([
                    ("code".into(), json!(diagnostic.code)),
                    ("level".into(), json!(diagnostic.level.as_str())),
                    ("message".into(), json!(diagnostic.message)),
                ]);
                if let Some(address) = diagnostic.address {
                    object.insert("address".into(), json!(address));
                }
                Value::Object(object)
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
