use glassvm_core::{
    Address, ControlFlow, ControlFlowKind, EventContext, EventKind, ExecutionEvent, IoChannel,
    IoDirection, IoObservation, MachineId, NativeEvent, StateLocation, StateSpace, StateWrite,
    TrapInfo, VersionStamp,
};
use serde_json::{Map, Value};

use crate::identity::{CHIP8_ID, SEMANTICS, schema};

pub struct Chip8NativeEventAdapter;

impl Chip8NativeEventAdapter {
    pub(crate) fn normalize(
        &self,
        context: &EventContext,
        native: &NativeEvent,
    ) -> Result<Vec<ExecutionEvent>, String> {
        validate_context(context)?;
        if native.schema != schema("chip8.event") {
            return Err(format!(
                "unsupported CHIP-8 native event schema {} version {}",
                native.schema.id, native.schema.version
            ));
        }
        validate_payload(native)?;

        let mut event = match native.kind.as_str() {
            "clear_screen" | "draw" | "scroll_down" | "scroll_up" | "scroll_right"
            | "scroll_left" | "hires_enabled" | "lores_enabled" => {
                let mut event = ExecutionEvent::from_context(context, EventKind::DisplayWrite);
                event.io.push(IoObservation {
                    port: "display_out".into(),
                    direction: IoDirection::Output,
                    channel: IoChannel::Display,
                    value: native.payload.clone(),
                });
                if native.kind == "draw"
                    && let Some(collision) =
                        native.payload.get("collision").and_then(Value::as_bool)
                {
                    event.io.push(IoObservation {
                        port: "collision_out".into(),
                        direction: IoDirection::Output,
                        channel: IoChannel::Collision,
                        value: Value::Bool(collision),
                    });
                }
                event
            }
            "jump" => control_flow_event(context, native, ControlFlowKind::Branch),
            "call" => control_flow_event(context, native, ControlFlowKind::Call),
            "return" => control_flow_event(context, native, ControlFlowKind::Return),
            "key_wait_entered" => ExecutionEvent::from_context(
                context,
                EventKind::Extension("chip8.key_wait_entered".into()),
            ),
            "key_wait_resolved" => {
                let mut event = ExecutionEvent::from_context(context, EventKind::InputSampled);
                event.io.push(IoObservation {
                    port: "keypad_in".into(),
                    direction: IoDirection::Input,
                    channel: IoChannel::Keypad,
                    value: native.payload.clone(),
                });
                event
            }
            "timer_set" => {
                let timer = native
                    .payload
                    .get("timer")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let value = native.payload.get("value").cloned();
                let mut event = ExecutionEvent::from_context(context, EventKind::TimerChanged);
                event.writes.push(StateWrite {
                    location: StateLocation::named(StateSpace::Timer, timer),
                    before: None,
                    after: value.clone(),
                });
                event.io.push(IoObservation {
                    port: if timer == "sound" {
                        "sound_out".into()
                    } else {
                        "timer_out".into()
                    },
                    direction: IoDirection::Output,
                    channel: if timer == "sound" {
                        IoChannel::Sound
                    } else {
                        IoChannel::Timer
                    },
                    value: value.unwrap_or(Value::Null),
                });
                event
            }
            "memory_write" => {
                let addr = native
                    .payload
                    .get("addr")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let len = native
                    .payload
                    .get("len")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let mut event = ExecutionEvent::from_context(context, EventKind::MemoryWrite);
                let mut location =
                    StateLocation::addressed(StateSpace::Memory, Address::new("memory", addr));
                location.width_bits = u16::try_from(len.saturating_mul(8)).ok();
                event.writes.push(StateWrite {
                    location,
                    before: None,
                    after: native.payload.get("after").cloned(),
                });
                event
            }
            "stack_push" | "stack_pop" => {
                ExecutionEvent::from_context(context, EventKind::StackChanged)
            }
            "invalid_opcode" => {
                let opcode = native
                    .payload
                    .get("opcode")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let mut event = ExecutionEvent::from_context(context, EventKind::Trap);
                event.trap = Some(TrapInfo {
                    code: format!("chip8.invalid_opcode.{opcode:04x}"),
                    message: format!("invalid CHIP-8 opcode 0x{opcode:04x}"),
                    fatal: true,
                });
                event
            }
            other => return Err(format!("unsupported CHIP-8 native event kind {other:?}")),
        };
        event
            .extensions
            .insert("chip8.native".into(), native.payload.clone());
        Ok(vec![event])
    }
}

fn validate_context(context: &EventContext) -> Result<(), String> {
    if context.arch != MachineId::from(CHIP8_ID) {
        return Err(format!(
            "CHIP-8 adapter cannot normalize foreign architecture {:?}",
            context.arch
        ));
    }
    if context.machine_version != VersionStamp::from(SEMANTICS) {
        return Err(format!(
            "CHIP-8 adapter cannot normalize machine version {:?}",
            context.machine_version
        ));
    }
    Ok(())
}

fn validate_payload(native: &NativeEvent) -> Result<(), String> {
    let kind = native.kind.as_str();
    match kind {
        "clear_screen" | "key_wait_entered" | "stack_push" | "stack_pop" | "hires_enabled"
        | "lores_enabled" => {
            exact_object(&native.payload, &[], kind)?;
        }
        "jump" | "call" => {
            let object = exact_object(&native.payload, &["from", "to"], kind)?;
            require_u16(object, "from", kind)?;
            require_u16(object, "to", kind)?;
        }
        "return" => {
            let object = exact_object(&native.payload, &["to"], kind)?;
            require_u16(object, "to", kind)?;
        }
        "draw" => {
            let object = exact_object(&native.payload, &["x", "y", "n", "collision"], kind)?;
            for field in ["x", "y", "n"] {
                require_u8(object, field, kind)?;
            }
            require_bool(object, "collision", kind)?;
        }
        "key_wait_resolved" => {
            let object = exact_object(&native.payload, &["key"], kind)?;
            let key = require_u8(object, "key", kind)?;
            if key > 0x0f {
                return Err(format!("CHIP-8 key {key} is outside 0..=15"));
            }
        }
        "timer_set" => {
            let object = exact_object(&native.payload, &["timer", "value"], kind)?;
            let timer = require_string(object, "timer", kind)?;
            if !matches!(timer, "delay" | "sound") {
                return Err(format!("unsupported CHIP-8 timer {timer:?}"));
            }
            require_u8(object, "value", kind)?;
        }
        "memory_write" => validate_memory_write(&native.payload)?,
        "scroll_down" | "scroll_up" => {
            let object = exact_object(&native.payload, &["rows"], kind)?;
            require_u8(object, "rows", kind)?;
        }
        "scroll_right" | "scroll_left" => {
            let object = exact_object(&native.payload, &["pixels"], kind)?;
            if require_u8(object, "pixels", kind)? != 4 {
                return Err(format!("CHIP-8 {kind} event must move exactly 4 pixels"));
            }
        }
        "invalid_opcode" => {
            let object = exact_object(&native.payload, &["pc", "opcode"], kind)?;
            require_u16(object, "pc", kind)?;
            require_u16(object, "opcode", kind)?;
        }
        other => return Err(format!("unsupported CHIP-8 native event kind {other:?}")),
    }
    Ok(())
}

fn validate_memory_write(payload: &Value) -> Result<(), String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "CHIP-8 memory_write payload must be an object".to_string())?;
    let compact = object.len() == 2 && object.contains_key("addr") && object.contains_key("len");
    let detailed = object.len() == 4
        && ["addr", "len", "before", "after"]
            .iter()
            .all(|field| object.contains_key(*field));
    if !compact && !detailed {
        return Err(
            "CHIP-8 memory_write payload must contain exactly addr/len or addr/len/before/after"
                .into(),
        );
    }
    require_u16(object, "addr", "memory_write")?;
    let len = usize::from(require_u16(object, "len", "memory_write")?);
    if detailed {
        for field in ["before", "after"] {
            let bytes = object
                .get(field)
                .and_then(Value::as_array)
                .ok_or_else(|| format!("CHIP-8 memory_write {field:?} must be a byte array"))?;
            if bytes.len() != len
                || bytes
                    .iter()
                    .any(|byte| byte.as_u64().is_none_or(|v| v > 255))
            {
                return Err(format!(
                    "CHIP-8 memory_write {field:?} must contain exactly {len} bytes"
                ));
            }
        }
    }
    Ok(())
}

fn exact_object<'a>(
    value: &'a Value,
    fields: &[&str],
    kind: &str,
) -> Result<&'a Map<String, Value>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("CHIP-8 {kind} event payload must be an object"))?;
    if object.len() != fields.len() || fields.iter().any(|field| !object.contains_key(*field)) {
        return Err(format!(
            "CHIP-8 {kind} event payload must contain exactly {:?}",
            fields
        ));
    }
    Ok(object)
}

fn require_u8(object: &Map<String, Value>, field: &str, kind: &str) -> Result<u8, String> {
    let value = require_u64(object, field, kind)?;
    u8::try_from(value).map_err(|_| format!("CHIP-8 {kind} {field:?} exceeds 8 bits"))
}

fn require_u16(object: &Map<String, Value>, field: &str, kind: &str) -> Result<u16, String> {
    let value = require_u64(object, field, kind)?;
    u16::try_from(value).map_err(|_| format!("CHIP-8 {kind} {field:?} exceeds 16 bits"))
}

fn require_u64(object: &Map<String, Value>, field: &str, kind: &str) -> Result<u64, String> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("CHIP-8 {kind} event requires unsigned {field:?}"))
}

fn require_bool(object: &Map<String, Value>, field: &str, kind: &str) -> Result<bool, String> {
    object
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("CHIP-8 {kind} event requires boolean {field:?}"))
}

fn require_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    kind: &str,
) -> Result<&'a str, String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("CHIP-8 {kind} event requires string {field:?}"))
}

pub(super) fn control_flow_event(
    context: &EventContext,
    native: &NativeEvent,
    kind: ControlFlowKind,
) -> ExecutionEvent {
    let from = native
        .payload
        .get("from")
        .and_then(Value::as_u64)
        .map(|value| Address::new("program", value))
        .or_else(|| context.pc.clone());
    let to = native
        .payload
        .get("to")
        .and_then(Value::as_u64)
        .map(|value| Address::new("program", value));
    let event_kind = match kind {
        ControlFlowKind::Call => EventKind::Call,
        ControlFlowKind::Return => EventKind::Return,
        _ => EventKind::BranchTaken,
    };
    let mut event = ExecutionEvent::from_context(context, event_kind);
    event.control_flow = Some(ControlFlow {
        kind,
        from,
        to,
        taken: true,
    });
    event
}
