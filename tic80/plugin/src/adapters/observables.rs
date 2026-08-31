use glassvm_core::{
    CapabilityBlob, MachineId, NativeObservation, ObservableAdapter, SchemaRef, SchemaVersion,
};
use serde_json::Value;

use crate::identity::MACHINE_ID;

pub struct Tic80ObservableAdapter;

impl ObservableAdapter for Tic80ObservableAdapter {
    fn machine_id(&self) -> MachineId {
        MachineId::from(MACHINE_ID)
    }

    fn normalize(&self, observations: &[NativeObservation]) -> Result<Vec<CapabilityBlob>, String> {
        observations
            .iter()
            .map(|observation| {
                validate_observation(observation)?;
                Ok(CapabilityBlob {
                    key: format!("tic80.{}", observation.name),
                    payload: observation.payload.clone(),
                })
            })
            .collect()
    }
}

fn validate_observation(observation: &NativeObservation) -> Result<(), String> {
    let expected_schema = match observation.name.as_str() {
        "framebuffer_rgba" => SchemaRef::new("tic80.framebuffer", SchemaVersion::V1),
        "frame_summary" => SchemaRef::new("tic80.execution", SchemaVersion::V1),
        name => return Err(format!("unsupported TIC-80 observation name {name:?}")),
    };
    if observation.schema != expected_schema {
        return Err(format!(
            "TIC-80 observation {:?} has unsupported schema {} version {}",
            observation.name, observation.schema.id, observation.schema.version
        ));
    }
    let payload = observation.payload.as_object().ok_or_else(|| {
        format!(
            "TIC-80 observation {:?} payload must be an object",
            observation.name
        )
    })?;
    match observation.name.as_str() {
        "framebuffer_rgba" => {
            let expected = ["bytes", "height", "sha256", "width"];
            if payload.len() != expected.len()
                || expected.iter().any(|key| !payload.contains_key(*key))
                || payload["width"].as_u64() != Some(tic80_core::WIDTH as u64)
                || payload["height"].as_u64() != Some(tic80_core::HEIGHT as u64)
                || !valid_sha256(&payload["sha256"])
                || !(payload["bytes"].is_null()
                    || payload["bytes"].as_array().is_some_and(|bytes| {
                        bytes.len() == tic80_core::WIDTH * tic80_core::HEIGHT * 4
                            && bytes.iter().all(|byte| {
                                byte.as_u64().is_some_and(|byte| byte <= u8::MAX.into())
                            })
                    }))
            {
                return Err("TIC-80 framebuffer observation payload is malformed".into());
            }
        }
        "frame_summary" => {
            let expected = ["frames", "language", "runtime", "trace_count", "traces"];
            let traces = payload["traces"].as_array();
            if payload.len() != expected.len()
                || expected.iter().any(|key| !payload.contains_key(*key))
                || payload["frames"].as_u64().is_none()
                || traces.is_none()
                || traces.is_some_and(|traces| {
                    payload["trace_count"].as_u64() != Some(traces.len() as u64)
                        || traces.iter().any(|trace| trace.as_str().is_none())
                })
                || payload["language"].as_str() != Some("lua")
                || payload["runtime"].as_str() != Some("lua54")
            {
                return Err("TIC-80 frame-summary observation payload is malformed".into());
            }
        }
        _ => unreachable!("observation name checked above"),
    }
    Ok(())
}

fn valid_sha256(value: &Value) -> bool {
    value.as_str().is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}
