//! Hexwell: Native static-analysis report adaptation for GlassVM.

use glassvm_core::{
    AnalyzeResult, CapabilityId, CapabilityOutput, MachineId, StaticAnalyzerBackend, caps,
};
use serde_json::json;

use crate::identity::MACHINE_ID;
use hexwell_core::WELL_COUNT;

pub struct HexwellStaticAnalyzerBackend;

impl StaticAnalyzerBackend for HexwellStaticAnalyzerBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(MACHINE_ID)
    }

    fn analyze_bytes(&self, artifact: &[u8]) -> Result<AnalyzeResult, String> {
        let report = hexwell_verifier::analyze_bytes(artifact)?;

        Ok(AnalyzeResult {
            capabilities: vec![
                capability(
                    "hexwell.static_profile",
                    json!({
                        "catalyst_count": report.structure.catalyst_count,
                        "opcode_counts": report.opcode_counts,
                        "selector_counts": report.selector_counts,
                        "ember_polarity": report.ember_polarity,
                        "transport_catalysts": report.opcode_counts[1] + report.opcode_counts[2] + report.opcode_counts[3],
                        "reaction_catalysts": report.opcode_counts[4] + report.opcode_counts[5] + report.opcode_counts[6] + report.opcode_counts[7] + report.opcode_counts[8] + report.opcode_counts[9],
                        "front_control_catalysts": report.opcode_counts[0xC] + report.opcode_counts[0xD] + report.opcode_counts[0xE],
                        "vent_catalysts": report.opcode_counts[0xF],
                        "unlike_neighbor_edges": report.unlike_neighbor_edges,
                        "neighbor_edges": report.neighbor_edges,
                        "family_edge_diversity": report.unlike_neighbor_edges as f64 / report.neighbor_edges as f64,
                        "boot_catalyst": {
                            "well": report.boot_well,
                            "byte": report.boot_byte,
                            "decoded": report.boot_decoded
                        }
                    }),
                ),
                capability(
                    caps::COVERAGE_SUMMARY,
                    json!({
                        "instructions_reached": 0,
                        "total_instructions": WELL_COUNT,
                        "coverage_ratio": 0.0,
                        "note": "activation reachability depends on synchronous fronts and dynamic concentrations"
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
