use glassvm_core::{ContentDigest, canonical_json_fingerprint};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SessionLifecycle {
    Fresh,
    Incremental,
    Terminal,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReplaySnapshot {
    pub(super) version: u32,
    pub(super) cart_sha256: String,
    pub(super) request_sha256: String,
    pub(super) input_history: Vec<u32>,
    pub(super) input_mask: u32,
    pub(super) input_override_pending: bool,
    pub(super) next_stimulus: usize,
    pub(super) lifecycle: SessionLifecycle,
    pub(super) payload_digest: ContentDigest,
}

pub(super) fn snapshot_digest(snapshot: &ReplaySnapshot) -> Result<ContentDigest, String> {
    let mut payload = serde_json::to_value(snapshot)
        .map_err(|error| format!("TIC-80 snapshot digest serialization failed: {error}"))?;
    payload
        .as_object_mut()
        .ok_or("TIC-80 snapshot digest payload is not an object")?
        .remove("payload_digest");
    canonical_json_fingerprint("glassvm.tic80.replay-snapshot.v3", &payload)
}
