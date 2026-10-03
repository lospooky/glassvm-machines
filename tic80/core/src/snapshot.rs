//! Native continuation state reconstructed through deterministic input replay.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSnapshot {
    pub ram: Vec<u8>,
    pub overlay_vram: Vec<u8>,
    pub active_video_bank: u8,
    pub frame: u64,
    pub input: u32,
    pub previous_input: u32,
    pub button_holds: [u32; 32],
    pub clip: [i32; 4],
    pub exit_requested: bool,
}
