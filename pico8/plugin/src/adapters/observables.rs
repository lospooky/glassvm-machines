use glassvm_core::{CapabilityBlob, MachineId, NativeObservation, ObservableAdapter, caps};
use serde_json::{Map, Value};

use crate::identity::{PICO8_ID, schema};

pub struct Pico8ObservableAdapter;

impl ObservableAdapter for Pico8ObservableAdapter {
    fn machine_id(&self) -> MachineId {
        MachineId::from(PICO8_ID)
    }

    fn normalize(&self, observations: &[NativeObservation]) -> Result<Vec<CapabilityBlob>, String> {
        observations
            .iter()
            .map(|observation| {
                let (schema_id, key) = match observation.name.as_str() {
                    "pico8.framebuffer_flat" => {
                        ("pico8.framebuffer", caps::DISPLAY_FRAMEBUFFER_FLAT)
                    }
                    "pico8.display_dims" => ("pico8.display_dims", caps::DISPLAY_DIMS),
                    "pico8.execution_summary" => {
                        ("pico8.execution_summary", "pico8.execution_summary")
                    }
                    "pico8.frame_hashes" => ("pico8.frame_hashes", "pico8.frame_hashes"),
                    "pico8.runtime_error" => ("pico8.runtime_error", "pico8.runtime_error"),
                    other => return Err(format!("unsupported PICO-8 observation {other:?}")),
                };
                if observation.schema != schema(schema_id) {
                    return Err(format!(
                        "PICO-8 observation {:?} requires schema {schema_id:?} version 1.0.0",
                        observation.name
                    ));
                }
                validate_payload(&observation.name, &observation.payload)?;
                Ok(CapabilityBlob {
                    key: key.into(),
                    payload: observation.payload.clone(),
                })
            })
            .collect()
    }
}

fn validate_payload(name: &str, payload: &Value) -> Result<(), String> {
    match name {
        "pico8.display_dims" => {
            let object = exact_object(payload, &["width", "height", "planes"], name)?;
            for field in ["width", "height", "planes"] {
                require_u64(object, field, name)?;
            }
        }
        "pico8.framebuffer_flat" => {
            let object = exact_object(payload, &["width", "height", "planes", "bytes"], name)?;
            for field in ["width", "height", "planes"] {
                require_u64(object, field, name)?;
            }
            require_byte_array(object, "bytes", name)?;
        }
        "pico8.execution_summary" => {
            let object = exact_object(
                payload,
                &[
                    "artifact_format",
                    "artifact_version",
                    "source_bytes",
                    "callback_hz",
                    "frames",
                    "draw_calls",
                    "audio_calls",
                    "print_calls",
                    "instruction_budget_per_frame",
                    "compatibility_tier",
                    "numeric_model",
                ],
                name,
            )?;
            for field in [
                "artifact_version",
                "source_bytes",
                "callback_hz",
                "frames",
                "draw_calls",
                "audio_calls",
                "print_calls",
                "instruction_budget_per_frame",
            ] {
                require_u64(object, field, name)?;
            }
            for field in ["artifact_format", "compatibility_tier", "numeric_model"] {
                require_string(object, field, name)?;
            }
        }
        "pico8.frame_hashes" => {
            let hashes = payload
                .as_array()
                .ok_or_else(|| "PICO-8 frame hashes must be an array".to_string())?;
            if hashes.iter().any(|hash| hash.as_str().is_none()) {
                return Err("PICO-8 frame hashes must contain strings".into());
            }
        }
        "pico8.runtime_error" => {
            let object = exact_object(payload, &["message"], name)?;
            require_string(object, "message", name)?;
        }
        _ => unreachable!("name matched before payload validation"),
    }
    Ok(())
}

fn exact_object<'a>(
    value: &'a Value,
    fields: &[&str],
    name: &str,
) -> Result<&'a Map<String, Value>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("PICO-8 observation {name:?} payload must be an object"))?;
    if object.len() != fields.len() || fields.iter().any(|field| !object.contains_key(*field)) {
        return Err(format!(
            "PICO-8 observation {name:?} payload must contain exactly {:?}",
            fields
        ));
    }
    Ok(object)
}

fn require_u64(object: &Map<String, Value>, field: &str, name: &str) -> Result<u64, String> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("PICO-8 observation {name:?} requires unsigned {field:?}"))
}

fn require_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    name: &str,
) -> Result<&'a str, String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("PICO-8 observation {name:?} requires string {field:?}"))
}

fn require_byte_array(object: &Map<String, Value>, field: &str, name: &str) -> Result<(), String> {
    let bytes = object
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("PICO-8 observation {name:?} requires byte array {field:?}"))?;
    if bytes
        .iter()
        .any(|byte| byte.as_u64().is_none_or(|value| value > 255))
    {
        return Err(format!(
            "PICO-8 observation {name:?} {field:?} contains a non-byte"
        ));
    }
    Ok(())
}
