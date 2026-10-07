use glassvm_core::{
    AnalyzeResult, CapabilityId, CapabilityOutput, MachineId, StaticAnalyzerBackend, caps,
};
use serde_json::json;

use crate::identity::CHIP8_ID;

pub struct Chip8StaticAnalyzerBackend;

impl StaticAnalyzerBackend for Chip8StaticAnalyzerBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(CHIP8_ID)
    }

    fn analyze_bytes(&self, artifact: &[u8]) -> Result<AnalyzeResult, String> {
        let report = chip8_verifier::verify_bytes(artifact);
        Ok(AnalyzeResult {
            capabilities: analysis_capabilities(&report),
        })
    }
}

pub(crate) fn analysis_capabilities(
    report: &chip8_verifier::VerifyReport,
) -> Vec<CapabilityOutput> {
    vec![
        capability(
            caps::VERIFIER_STRUCTURAL,
            json!({
                "reachable_instruction_count": report.structural.reachable_instruction_count,
                "basic_block_count": report.structural.basic_block_count,
                "loop_count": report.structural.loop_count,
                "max_cfg_depth": report.structural.max_cfg_depth,
                "reachable_ratio": report.structural.reachable_ratio,
                "estimated_code_bytes": report.structural.estimated_code_bytes,
                "estimated_data_bytes": report.structural.estimated_data_bytes,
            }),
        ),
        capability(
            caps::VERIFIER_BEHAVIORAL,
            json!({
                "contains_draw": report.behavioral.contains_draw,
                "contains_key_input": report.behavioral.contains_key_input,
                "contains_timers": report.behavioral.contains_timers,
                "contains_sound": report.behavioral.contains_sound,
                "contains_collision_detection": report.behavioral.contains_collision_detection,
                "contains_randomness": report.behavioral.contains_randomness,
            }),
        ),
    ]
}

fn capability(id: &str, value: serde_json::Value) -> CapabilityOutput {
    let id = CapabilityId::new(id).expect("static analyzer capability ID");
    CapabilityOutput {
        schema: id.schema(),
        value,
    }
}
