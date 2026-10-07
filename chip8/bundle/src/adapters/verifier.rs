use glassvm_core::{
    CapabilityId, CapabilityOutput, MachineId, VerifierBackend, VerifyResult, caps,
};
use serde_json::{Value, json};

use crate::identity::CHIP8_ID;

use super::static_analyzer::analysis_capabilities;

pub struct Chip8VerifierBackend;

impl VerifierBackend for Chip8VerifierBackend {
    fn machine_id(&self) -> MachineId {
        MachineId(CHIP8_ID.to_string())
    }

    fn verify_bytes(&self, rom_bytes: &[u8]) -> Result<VerifyResult, String> {
        let report = chip8_verifier::verify_bytes(rom_bytes);

        let diagnostics: Vec<Value> = report
            .diagnostics
            .iter()
            .map(|d| {
                json!({
                    "code": d.code,
                    "level": format!("{:?}", d.level).to_lowercase(),
                    "addr": d.addr,
                    "message": d.message,
                })
            })
            .collect();

        let severity_max = if report
            .diagnostics
            .iter()
            .any(|d| d.level == chip8_verifier::Level::Error)
        {
            "error"
        } else if report
            .diagnostics
            .iter()
            .any(|d| d.level == chip8_verifier::Level::Warning)
        {
            "warning"
        } else {
            "info"
        }
        .to_string();

        let mut verify_caps = vec![CapabilityOutput {
            schema: CapabilityId::new(caps::VERIFIER_EXTENSION_LEVEL)
                .expect("verifier capability ID")
                .schema(),
            value: json!(format!("{}", report.extension)),
        }];
        verify_caps.extend(analysis_capabilities(&report));

        Ok(VerifyResult {
            severity_max,
            diagnostics,
            capabilities: verify_caps,
        })
    }
}
