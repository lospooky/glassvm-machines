use glassvm_core::{
    EventContext, EventKind, ExecutionEvent, MachineId, NativeEvent, NativeEventAdapter, SchemaRef,
    SchemaVersion, VersionStamp,
};
use serde_json::json;
use tic80_core::Tic80Event;

use crate::identity::{MACHINE_ID, SEMANTICS, schema};

pub struct Tic80NativeEventAdapter;

impl Tic80NativeEventAdapter {
    pub(crate) fn from_core(event: &Tic80Event) -> NativeEvent {
        let (kind, payload) = match event {
            Tic80Event::FrameCompleted { frame } => ("frame", json!({"frame": frame})),
            Tic80Event::InputSampled { mask } => ("input", json!({"mask": mask})),
            Tic80Event::Trace { message } => ("trace", json!({"message": message})),
        };
        NativeEvent {
            schema: schema("tic80.event"),
            kind: kind.into(),
            payload,
        }
    }
}

impl NativeEventAdapter for Tic80NativeEventAdapter {
    fn machine_id(&self) -> MachineId {
        MachineId::from(MACHINE_ID)
    }

    fn normalize(
        &self,
        context: &EventContext,
        native: &NativeEvent,
    ) -> Result<Vec<ExecutionEvent>, String> {
        if context.arch != MachineId::from(MACHINE_ID)
            || context.machine_version != VersionStamp::from(SEMANTICS)
        {
            return Err("TIC-80 native-event context has a foreign machine identity".into());
        }
        if native.schema != SchemaRef::new("tic80.event", SchemaVersion::V1) {
            return Err(format!(
                "unsupported TIC-80 native event schema {} version {}",
                native.schema.id, native.schema.version
            ));
        }
        validate_payload(native)?;
        let kind = match native.kind.as_str() {
            "frame" => EventKind::FrameCompleted,
            "input" => EventKind::InputSampled,
            "trace" => EventKind::Extension("tic80.trace".into()),
            other => return Err(format!("unsupported TIC-80 native event kind {other:?}")),
        };
        let mut event = ExecutionEvent::from_context(context, kind);
        event
            .extensions
            .insert(format!("tic80.{}", native.kind), native.payload.clone());
        Ok(vec![event])
    }
}

fn validate_payload(native: &NativeEvent) -> Result<(), String> {
    let object = native
        .payload
        .as_object()
        .ok_or_else(|| format!("TIC-80 {} payload must be an object", native.kind))?;
    match native.kind.as_str() {
        "input"
            if object.len() != 1
                || object["mask"]
                    .as_u64()
                    .is_none_or(|mask| mask > u32::MAX.into()) =>
        {
            return Err("TIC-80 input payload requires exactly a u32 mask".into());
        }
        "frame" if object.len() != 1 || object["frame"].as_u64().is_none() => {
            return Err("TIC-80 frame payload requires exactly an integer frame".into());
        }
        "trace" if object.len() != 1 || object["message"].as_str().is_none() => {
            return Err("TIC-80 trace payload requires exactly a string message".into());
        }
        _ => {}
    }
    Ok(())
}
