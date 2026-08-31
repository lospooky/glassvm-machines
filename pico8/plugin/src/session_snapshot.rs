use glassvm_core::{ContentDigest, ExecutionRequest, ReplayStimulus, canonical_json_bytes};
use pico8_core::RuntimeSnapshot;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SessionLifecycle {
    Fresh,
    Incremental,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Pico8Continuation {
    pub(super) schema_version: u32,
    pub(super) machine_version: String,
    pub(super) emulator_version: String,
    pub(super) artifact_digest: ContentDigest,
    pub(super) request_digest: ContentDigest,
    pub(super) runtime: RuntimeSnapshot,
    pub(super) stimuli: Vec<ReplayStimulus>,
    pub(super) applied_stimuli: u64,
    pub(super) lifecycle: SessionLifecycle,
    pub(super) runtime_error: Option<String>,
    pub(super) integrity_digest: ContentDigest,
}

#[derive(Serialize)]
pub(super) struct Pico8ContinuationIntegrity<'a> {
    pub(super) domain: &'static str,
    pub(super) schema_version: u32,
    pub(super) machine_version: &'a str,
    pub(super) emulator_version: &'a str,
    pub(super) artifact_digest: ContentDigest,
    pub(super) request_digest: ContentDigest,
    pub(super) runtime: &'a RuntimeSnapshot,
    pub(super) stimuli: &'a [ReplayStimulus],
    pub(super) applied_stimuli: u64,
    pub(super) lifecycle: SessionLifecycle,
    pub(super) runtime_error: Option<&'a str>,
}

pub(super) fn request_digest(request: &ExecutionRequest) -> Result<ContentDigest, String> {
    Ok(ContentDigest::sha256(&canonical_json_bytes(request)?))
}

pub(super) fn continuation_integrity(
    continuation: &Pico8Continuation,
) -> Result<ContentDigest, String> {
    let integrity = Pico8ContinuationIntegrity {
        domain: "pico8.continuation.integrity.v2",
        schema_version: continuation.schema_version,
        machine_version: &continuation.machine_version,
        emulator_version: &continuation.emulator_version,
        artifact_digest: continuation.artifact_digest,
        request_digest: continuation.request_digest,
        runtime: &continuation.runtime,
        stimuli: &continuation.stimuli,
        applied_stimuli: continuation.applied_stimuli,
        lifecycle: continuation.lifecycle,
        runtime_error: continuation.runtime_error.as_deref(),
    };
    Ok(ContentDigest::sha256(&canonical_json_bytes(&integrity)?))
}

pub(super) fn observable_state_bytes(snapshot: &RuntimeSnapshot) -> Result<Vec<u8>, String> {
    canonical_json_bytes(snapshot)
}
