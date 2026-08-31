use glassvm_core::{
    EventContext, EventKind, ExecutionEvent, IoChannel, IoDirection, IoObservation, MachineId,
    NativeEvent, NativeEventAdapter, VersionStamp,
};
use serde_json::{Map, Value};

use crate::identity::{MACHINE_VERSION, PICO8_ID, schema};

pub struct Pico8NativeEventAdapter;

impl Pico8NativeEventAdapter {
    pub(crate) fn from_execution(kind: &EventKind, payload: Value) -> Result<NativeEvent, String> {
        let kind = match kind {
            EventKind::RunStarted => "run_started",
            EventKind::RunHalted => "run_halted",
            EventKind::RunCrashed => "run_crashed",
            EventKind::FrameCompleted => "frame_completed",
            EventKind::InputSampled => "input_sampled",
            EventKind::SoundEmitted => "audio_command",
            EventKind::DisplayWrite => "display_write",
            EventKind::Trap => "runtime_error",
            other => {
                return Err(format!(
                    "PICO-8 cannot encode normalized event kind {other:?} as a native event"
                ));
            }
        };
        Ok(NativeEvent {
            schema: schema("pico8.event"),
            kind: kind.into(),
            payload,
        })
    }
}

impl NativeEventAdapter for Pico8NativeEventAdapter {
    fn machine_id(&self) -> MachineId {
        MachineId::from(PICO8_ID)
    }

    fn normalize(
        &self,
        context: &EventContext,
        native: &NativeEvent,
    ) -> Result<Vec<ExecutionEvent>, String> {
        validate_context(context)?;
        if native.schema != schema("pico8.event") {
            return Err(format!(
                "unsupported PICO-8 native event schema {} version {}",
                native.schema.id, native.schema.version
            ));
        }
        validate_payload(native)?;
        let kind = match native.kind.as_str() {
            "run_started" => EventKind::RunStarted,
            "run_halted" => EventKind::RunHalted,
            "run_crashed" => EventKind::RunCrashed,
            "frame_completed" => EventKind::FrameCompleted,
            "input_sampled" => EventKind::InputSampled,
            "audio_command" => EventKind::SoundEmitted,
            "display_write" => EventKind::DisplayWrite,
            "runtime_error" => EventKind::Trap,
            other => return Err(format!("unsupported PICO-8 native event kind {other:?}")),
        };
        let mut event = ExecutionEvent::from_context(context, kind);
        if event.kind == EventKind::FrameCompleted {
            event.io.push(IoObservation {
                port: "display_out".into(),
                direction: IoDirection::Output,
                channel: IoChannel::Display,
                value: native.payload.clone(),
            });
        } else if event.kind == EventKind::InputSampled {
            event.io.push(IoObservation {
                port: "controllers_in".into(),
                direction: IoDirection::Input,
                channel: IoChannel::Keypad,
                value: native.payload.clone(),
            });
        }
        event
            .extensions
            .insert("pico8.native".into(), native.payload.clone());
        Ok(vec![event])
    }
}

fn validate_context(context: &EventContext) -> Result<(), String> {
    if context.arch != MachineId::from(PICO8_ID) {
        return Err(format!(
            "PICO-8 adapter cannot normalize foreign architecture {:?}",
            context.arch
        ));
    }
    if context.machine_version != VersionStamp::from(MACHINE_VERSION) {
        return Err(format!(
            "PICO-8 adapter cannot normalize machine version {:?}",
            context.machine_version
        ));
    }
    Ok(())
}

fn validate_payload(native: &NativeEvent) -> Result<(), String> {
    let kind = native.kind.as_str();
    match kind {
        "run_started" | "run_halted" => {
            exact_object(&native.payload, &[], kind)?;
        }
        "run_crashed" => {
            let object = exact_object(&native.payload, &["error", "fatal"], kind)?;
            require_string(object, "error", kind)?;
            require_bool(object, "fatal", kind)?;
        }
        "frame_completed" => validate_frame_payload(&native.payload)?,
        "input_sampled" => {
            let object = exact_object(&native.payload, &["port", "mask"], kind)?;
            if require_string(object, "port", kind)? != "controllers_in" {
                return Err("PICO-8 input event targets an unsupported port".into());
            }
            let mask = require_u64(object, "mask", kind)?;
            if mask > u64::from(u16::MAX) {
                return Err(format!("PICO-8 input mask {mask:#x} exceeds 16 bits"));
            }
        }
        "audio_command" => {
            let object =
                exact_object(&native.payload, &["channel", "frequency", "duration"], kind)?;
            require_u64(object, "channel", kind)?;
            require_number(object, "frequency", kind)?;
            require_number(object, "duration", kind)?;
        }
        "display_write" => {
            let object = exact_object(&native.payload, &["x", "y", "color"], kind)?;
            for field in ["x", "y", "color"] {
                require_u64(object, field, kind)?;
            }
        }
        "runtime_error" => {
            let object = exact_object(&native.payload, &["message"], kind)?;
            require_string(object, "message", kind)?;
        }
        other => return Err(format!("unsupported PICO-8 native event kind {other:?}")),
    }
    Ok(())
}

fn validate_frame_payload(payload: &Value) -> Result<(), String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "PICO-8 frame_completed payload must be an object".to_string())?;
    if object.is_empty() {
        return Ok(());
    }
    let hashes = ["width", "height", "frame_hash"];
    let full = ["width", "height", "planes", "palette_indices", "frame_hash"];
    if object.len() == hashes.len() && hashes.iter().all(|field| object.contains_key(*field)) {
        require_u64(object, "width", "frame_completed")?;
        require_u64(object, "height", "frame_completed")?;
        require_string(object, "frame_hash", "frame_completed")?;
        return Ok(());
    }
    if object.len() == full.len() && full.iter().all(|field| object.contains_key(*field)) {
        for field in ["width", "height", "planes"] {
            require_u64(object, field, "frame_completed")?;
        }
        require_byte_array(object, "palette_indices", "frame_completed")?;
        require_string(object, "frame_hash", "frame_completed")?;
        return Ok(());
    }
    Err("PICO-8 frame_completed payload has an unsupported shape".into())
}

fn exact_object<'a>(
    value: &'a Value,
    fields: &[&str],
    kind: &str,
) -> Result<&'a Map<String, Value>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("PICO-8 {kind} event payload must be an object"))?;
    if object.len() != fields.len() || fields.iter().any(|field| !object.contains_key(*field)) {
        return Err(format!(
            "PICO-8 {kind} event payload must contain exactly {:?}",
            fields
        ));
    }
    Ok(object)
}

fn require_u64(object: &Map<String, Value>, field: &str, kind: &str) -> Result<u64, String> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("PICO-8 {kind} event requires unsigned {field:?}"))
}

fn require_bool(object: &Map<String, Value>, field: &str, kind: &str) -> Result<bool, String> {
    object
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("PICO-8 {kind} event requires boolean {field:?}"))
}

fn require_number(object: &Map<String, Value>, field: &str, kind: &str) -> Result<(), String> {
    if object.get(field).is_none_or(|value| !value.is_number()) {
        return Err(format!("PICO-8 {kind} event requires numeric {field:?}"));
    }
    Ok(())
}

fn require_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    kind: &str,
) -> Result<&'a str, String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("PICO-8 {kind} event requires string {field:?}"))
}

fn require_byte_array(object: &Map<String, Value>, field: &str, kind: &str) -> Result<(), String> {
    let bytes = object
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("PICO-8 {kind} event requires byte array {field:?}"))?;
    if bytes
        .iter()
        .any(|byte| byte.as_u64().is_none_or(|value| value > 255))
    {
        return Err(format!("PICO-8 {kind} event {field:?} contains a non-byte"));
    }
    Ok(())
}
