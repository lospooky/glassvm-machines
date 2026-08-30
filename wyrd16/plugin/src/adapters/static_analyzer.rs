//! Conversion from the native analysis report into GlassVM capabilities.

use glassvm_core::{
    AnalyzeResult, CapabilityId, CapabilityOutput, MachineId, StaticAnalyzerBackend, caps,
};
use serde_json::json;

use crate::identity::MACHINE_ID;

pub struct Wyrd16StaticAnalyzerBackend;

impl StaticAnalyzerBackend for Wyrd16StaticAnalyzerBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(MACHINE_ID)
    }

    fn analyze_bytes(&self, artifact: &[u8]) -> Result<AnalyzeResult, String> {
        let report = wyrd16_verifier::analyze_bytes(artifact)?;
        Ok(AnalyzeResult {
            capabilities: vec![
                capability(
                    "wyrd16.static_profile",
                    json!({
                        "rune_count": report.structure.rune_count,
                        "opcode_counts": report.opcode_counts,
                        "drawing_runes": report.drawing_runes,
                        "control_flow_runes": report.control_flow_runes,
                        "memory_write_runes": report.memory_write_runes,
                        "entropy_runes": report.entropy_runes,
                        "input_runes": report.input_runes,
                        "reserved_charms": report.reserved_charms
                    }),
                ),
                capability(
                    caps::COVERAGE_SUMMARY,
                    json!({
                        "instructions_reached": 0,
                        "total_instructions": report.structure.rune_count,
                        "coverage_ratio": 0.0,
                        "note": "dynamic reachability is intentionally not inferred across mutable code"
                    }),
                ),
            ],
        })
    }
}

fn capability(id: &str, value: serde_json::Value) -> CapabilityOutput {
    let id = CapabilityId::new(id).expect("static analyzer capability ID");
    CapabilityOutput {
        schema: id.schema(),
        value,
    }
}
