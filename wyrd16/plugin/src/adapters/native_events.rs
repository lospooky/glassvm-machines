//! Native-event normalization.

use glassvm_core::{
    EventContext, EventKind, ExecutionEvent, MachineId, NativeEvent, TrapInfo, VersionStamp,
};
use serde_json::{Map, Value};

use crate::identity::{MACHINE_ID, MACHINE_VERSION, schema};

pub struct Wyrd16NativeEventAdapter;

impl Wyrd16NativeEventAdapter {
    pub(crate) fn normalize(
        &self,
        context: &EventContext,
        native: &NativeEvent,
    ) -> Result<Vec<ExecutionEvent>, String> {
        validate_context(context)?;
        if native.schema != schema("wyrd16.event") {
            return Err(format!(
                "unsupported Wyrd-16 native event schema {}",
                native.schema.id
            ));
        }
        validate_payload(context, native)?;
        let kind = match native.kind.as_str() {
            "instruction" => EventKind::InstructionDecoded,
            "register" | "random" => EventKind::RegisterWrite,
            "pixel" | "line" | "weave" | "clear" | "palette" => EventKind::DisplayWrite,
            "jump" | "branch" => EventKind::BranchTaken,
            "load" => EventKind::MemoryRead,
            "store" => EventKind::MemoryWrite,
            "key" => EventKind::InputSampled,
            "halt" => EventKind::RunHalted,
            other => return Err(format!("unsupported Wyrd-16 native event kind {other:?}")),
        };
        let mut event = ExecutionEvent::from_context(context, kind);
        if native.kind == "halt" {
            event.trap = Some(TrapInfo {
                code: "W16_HALT".into(),
                message: "HALT charm".into(),
                fatal: false,
            });
        }
        event
            .extensions
            .insert("wyrd16.native".into(), native.payload.clone());
        Ok(vec![event])
    }
}

fn validate_context(context: &EventContext) -> Result<(), String> {
    if context.arch != MachineId::from(MACHINE_ID) {
        return Err(format!(
            "Wyrd-16 adapter cannot normalize foreign architecture {:?}",
            context.arch
        ));
    }
    if context.machine_version != VersionStamp::from(MACHINE_VERSION) {
        return Err(format!(
            "Wyrd-16 adapter cannot normalize machine version {:?}",
            context.machine_version
        ));
    }
    Ok(())
}

fn validate_payload(context: &EventContext, native: &NativeEvent) -> Result<(), String> {
    if !matches!(
        native.kind.as_str(),
        "instruction"
            | "register"
            | "random"
            | "pixel"
            | "line"
            | "weave"
            | "clear"
            | "palette"
            | "jump"
            | "branch"
            | "load"
            | "store"
            | "key"
            | "halt"
    ) {
        return Err(format!(
            "unsupported Wyrd-16 native event kind {:?}",
            native.kind
        ));
    }
    let object = exact_object(&native.payload, &["opcode", "cursor", "ink"], &native.kind)?;
    let opcode = require_u64(object, "opcode", &native.kind)?;
    if opcode > 0x0f {
        return Err(format!("Wyrd-16 opcode {opcode} exceeds 4 bits"));
    }
    let instruction = context
        .instruction
        .as_ref()
        .ok_or("Wyrd-16 native events require an instruction reference")?;
    if instruction.encoding != "wyrd16.rune16.be" {
        return Err(format!(
            "Wyrd-16 instruction encoding {:?} is not canonical",
            instruction.encoding
        ));
    }
    let instruction_bytes: [u8; 2] = instruction
        .bytes
        .as_slice()
        .try_into()
        .map_err(|_| "Wyrd-16 instruction reference must contain exactly two bytes")?;
    let instruction_word = u16::from_be_bytes(instruction_bytes);
    let instruction_opcode = u64::from(instruction_word >> 12);
    if opcode != instruction_opcode {
        return Err(format!(
            "Wyrd-16 payload opcode {opcode:#x} contradicts instruction word {instruction_word:#06x}"
        ));
    }
    let opcode_matches_kind = match native.kind.as_str() {
        "instruction" => !matches!(instruction_word, 0x0001 | 0x0002),
        "halt" => instruction_word == 0x0001,
        "clear" => instruction_word == 0x0002,
        "register" => (0x1..=0x4).contains(&opcode),
        "load" => opcode == 0x5,
        "store" => opcode == 0x6,
        "jump" => opcode == 0x7,
        "branch" => opcode == 0x8,
        "random" => opcode == 0x9,
        "key" => opcode == 0xA,
        "pixel" => opcode == 0xC,
        "line" => opcode == 0xD,
        "palette" => opcode == 0xE,
        "weave" => opcode == 0xF,
        _ => unreachable!("native kind was checked above"),
    };
    if !opcode_matches_kind {
        return Err(format!(
            "Wyrd-16 native event kind {:?} contradicts opcode {opcode:#x}",
            native.kind
        ));
    }
    let cursor = object
        .get("cursor")
        .and_then(Value::as_array)
        .ok_or_else(|| "Wyrd-16 event cursor must be an array".to_string())?;
    if cursor.len() != 2
        || cursor
            .iter()
            .any(|value| value.as_u64().is_none_or(|coordinate| coordinate >= 64))
    {
        return Err("Wyrd-16 event cursor must contain exactly two six-bit coordinates".into());
    }
    let ink = require_u64(object, "ink", &native.kind)?;
    if ink > 0x0f {
        return Err(format!("Wyrd-16 ink {ink} exceeds 4 bits"));
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
        .ok_or_else(|| format!("Wyrd-16 {kind} event payload must be an object"))?;
    if object.len() != fields.len() || fields.iter().any(|field| !object.contains_key(*field)) {
        return Err(format!(
            "Wyrd-16 {kind} event payload must contain exactly {:?}",
            fields
        ));
    }
    Ok(object)
}

fn require_u64(object: &Map<String, Value>, field: &str, kind: &str) -> Result<u64, String> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("Wyrd-16 {kind} event requires unsigned {field:?}"))
}
