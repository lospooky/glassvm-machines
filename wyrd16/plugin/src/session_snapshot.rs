//! GlassVM-bound session snapshot envelope.

use std::collections::BTreeSet;

use glassvm_core::{ContentDigest, ExecutionRequest, canonical_json_fingerprint};
use serde::{Deserialize, Serialize};
use wyrd16_core::MachineState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionLifecycle {
    Fresh,
    Incremental,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionSnapshot {
    pub format: String,
    pub version: u16,
    pub machine_version: String,
    pub emulator_version: String,
    pub request: ExecutionRequest,
    pub initial: MachineState,
    pub state: MachineState,
    pub coverage: BTreeSet<u16>,
    pub opcode_counts: [u64; 16],
    pub display_writes: u64,
    pub changed_pixels: u64,
    pub palette_writes: u64,
    pub next_scheduled_input: usize,
    pub next_sequence: u64,
    pub lifecycle: SessionLifecycle,
    pub frame_active: bool,
    pub cycles_into_frame: u32,
    pub payload_digest: ContentDigest,
}

pub(crate) fn snapshot_digest(snapshot: &SessionSnapshot) -> Result<ContentDigest, String> {
    let mut payload = serde_json::to_value(snapshot)
        .map_err(|error| format!("encode Wyrd-16 snapshot payload: {error}"))?;
    payload
        .as_object_mut()
        .ok_or("Wyrd-16 snapshot payload is not an object")?
        .remove("payload_digest");
    canonical_json_fingerprint("glassvm.wyrd16.session-snapshot.v4", &payload)
}
