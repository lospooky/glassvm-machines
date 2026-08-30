//! Static artifact profiling without execution.

use wyrd16_core::validate_artifact;

use crate::{AnalysisReport, ArtifactStructure, InspectedArtifact};

pub fn analyze_bytes(bytes: &[u8]) -> Result<AnalysisReport, String> {
    validate_artifact(bytes).map_err(|error| error.to_string())?;
    let artifact = InspectedArtifact::inspect(bytes);
    let mut opcode_counts = [0_u64; 16];
    let mut reserved_charms = 0_u64;
    for rune in &artifact.runes {
        opcode_counts[rune.op as usize] += 1;
        if rune.op == 0 && rune.addr12() > 3 {
            reserved_charms += 1;
        }
    }
    Ok(AnalysisReport {
        structure: ArtifactStructure::from(&artifact),
        drawing_runes: opcode_counts[0xC] + opcode_counts[0xD] + opcode_counts[0xF],
        control_flow_runes: opcode_counts[0x7] + opcode_counts[0x8],
        memory_write_runes: opcode_counts[0x6],
        entropy_runes: opcode_counts[0x9],
        input_runes: opcode_counts[0xA],
        reserved_charms,
        opcode_counts,
    })
}
