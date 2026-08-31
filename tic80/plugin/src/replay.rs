//! Recorded-gamepad validation and resolved episode identity.

use glassvm_core::{
    ExecutionRequest, ResolvedEpisodeContext, VersionStamp, VersionedComponentIdentity,
    component_identity, fixed_body_action_schema, fixed_body_identity,
    materialize_open_loop_episode_context,
};
use serde_json::json;

use crate::contract::contract;

fn recorded_gamepad_policy_identity() -> VersionedComponentIdentity {
    component_identity(
        VersionStamp::from(env!("CARGO_PKG_VERSION")),
        "tic80.input.recorded_gamepad_mask.v1",
        "tic80.input.policy",
        json!({"coordinate": "frame_start", "port": "gamepad_in", "width_bits": 32}),
    )
}

pub(crate) fn resolve_episode_context(
    request: &ExecutionRequest,
) -> Result<ResolvedEpisodeContext, String> {
    let body = contract().default_body;
    let identity = fixed_body_identity(&body, VersionStamp::from(env!("CARGO_PKG_VERSION")));
    materialize_open_loop_episode_context(
        request.episode.as_ref(),
        &request.stimuli,
        &identity,
        &fixed_body_action_schema(&body),
        Some(&recorded_gamepad_policy_identity()),
    )
}

pub(crate) fn validate_stimuli(request: &ExecutionRequest) -> Result<(), String> {
    let mut previous_frame = None;
    for (index, stimulus) in request.stimuli.iter().enumerate() {
        if stimulus.ordinal != index as u64 {
            return Err(format!(
                "TIC-80 stimulus at index {index} has ordinal {}; expected {index}",
                stimulus.ordinal
            ));
        }
        if stimulus.port != "gamepad_in" {
            return Err(format!(
                "TIC-80 stimulus {} targets unsupported port {:?}",
                stimulus.ordinal, stimulus.port
            ));
        }
        if stimulus.coordinate.step.is_some() || stimulus.coordinate.cycle_or_tick.is_some() {
            return Err(format!(
                "TIC-80 stimulus {} must use a frame-start coordinate only",
                stimulus.ordinal
            ));
        }
        let frame = stimulus.coordinate.frame.ok_or_else(|| {
            format!(
                "TIC-80 stimulus {} has no frame coordinate",
                stimulus.ordinal
            )
        })?;
        if frame >= request.config.max_frames {
            return Err(format!(
                "TIC-80 stimulus {} targets frame {frame}, outside max_frames {}",
                stimulus.ordinal, request.config.max_frames
            ));
        }
        if previous_frame.is_some_and(|previous| frame < previous) {
            return Err("TIC-80 stimuli must be ordered by nondecreasing frame".into());
        }
        previous_frame = Some(frame);
        let object = stimulus.value.as_object().ok_or_else(|| {
            format!(
                "TIC-80 stimulus {} value must be an object",
                stimulus.ordinal
            )
        })?;
        if object.len() != 1 || !object.contains_key("mask") {
            return Err(format!(
                "TIC-80 stimulus {} must contain exactly one mask field",
                stimulus.ordinal
            ));
        }
        let mask = object["mask"].as_u64().ok_or_else(|| {
            format!(
                "TIC-80 stimulus {} must contain an unsigned mask",
                stimulus.ordinal
            )
        })?;
        u32::try_from(mask).map_err(|_| {
            format!(
                "TIC-80 stimulus {} mask {mask:#x} exceeds four packed gamepads",
                stimulus.ordinal
            )
        })?;
    }
    Ok(())
}
