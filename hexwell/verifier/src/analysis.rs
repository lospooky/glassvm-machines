//! Catalyst family, selector, and neighborhood analysis.

use hexwell_core::{Catalyst, GRID_WIDTH, WELL_COUNT, neighbor, validate_artifact};

use crate::{AnalysisReport, ArtifactStructure};

pub fn analyze_bytes(bytes: &[u8]) -> Result<AnalysisReport, String> {
    validate_artifact(bytes).map_err(|error| error.to_string())?;
    let mut opcode_counts = [0_u64; 16];
    let mut selector_counts = [0_u64; 8];
    let mut ember_polarity = 0_u64;
    for byte in bytes {
        let catalyst = Catalyst::decode(*byte);
        opcode_counts[catalyst.family as usize] += 1;
        selector_counts[catalyst.selector() as usize] += 1;
        ember_polarity += u64::from(catalyst.polarity());
    }
    let mut unlike_neighbor_edges = 0_u64;
    let mut neighbor_edges = 0_u64;
    for well in 0..WELL_COUNT {
        let family = Catalyst::decode(bytes[well]).family;
        for direction in 0..3 {
            let adjacent = neighbor(well, direction);
            neighbor_edges += 1;
            unlike_neighbor_edges += u64::from(Catalyst::decode(bytes[adjacent]).family != family);
        }
    }
    let boot_well = GRID_WIDTH * 8 + 8;
    let boot = Catalyst::decode(bytes[boot_well]);
    Ok(AnalysisReport {
        structure: ArtifactStructure {
            catalyst_count: WELL_COUNT,
        },
        opcode_counts,
        selector_counts,
        ember_polarity,
        unlike_neighbor_edges,
        neighbor_edges,
        boot_well,
        boot_byte: boot.byte,
        boot_decoded: boot.disassemble(),
    })
}
