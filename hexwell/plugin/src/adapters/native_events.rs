//! Hexwell: Native-event normalization into the GlassVM event vocabulary.

use glassvm_core::{
    Address, ControlFlow, ControlFlowKind, EventContext, EventKind, ExecutionEvent, IoChannel,
    IoDirection, IoObservation, MachineId, NativeEvent, VersionStamp,
};
use serde_json::{Map, Value};

use crate::identity::{MACHINE_ID, MACHINE_VERSION, schema};

pub struct HexwellNativeEventAdapter;

impl HexwellNativeEventAdapter {
    pub(crate) fn normalize(
        &self,
        context: &EventContext,
        native: &NativeEvent,
    ) -> Result<Vec<ExecutionEvent>, String> {
        validate_context(context)?;
        if native.schema != schema("hexwell.event") {
            return Err(format!(
                "unsupported Hexwell native event schema {} version {}",
                native.schema.id, native.schema.version
            ));
        }
        validate_payload(native)?;
        let kind = match native.kind.as_str() {
            "tide" => EventKind::InputApplied,
            "feed" => EventKind::Extension("hexwell.tide_feed".into()),
            "frame" => EventKind::FrameCompleted,
            "quenched" => EventKind::RunHalted,
            "sweep" => EventKind::Extension("hexwell.sweep_committed".into()),
            "catalyst" => EventKind::Extension("hexwell.catalyst_fired".into()),
            other => return Err(format!("unsupported Hexwell native event kind {other:?}")),
        };
        let mut event = ExecutionEvent::from_context(context, kind);
        match native.kind.as_str() {
            "tide" => event.io.push(IoObservation {
                port: "tide_in".into(),
                direction: IoDirection::Input,
                channel: IoChannel::Extension("matter_flux".into()),
                value: native.payload.clone(),
            }),
            "feed" => {
                event.io.push(IoObservation {
                    port: "tide_in".into(),
                    direction: IoDirection::Input,
                    channel: IoChannel::Extension("matter_flux".into()),
                    value: native.payload.clone(),
                });
                event.io.push(IoObservation {
                    port: "matter_out".into(),
                    direction: IoDirection::Output,
                    channel: IoChannel::Extension("matter_ledger".into()),
                    value: native.payload.clone(),
                });
            }
            "frame" => event.io.push(IoObservation {
                port: "reactor_out".into(),
                direction: IoDirection::Output,
                channel: IoChannel::Display,
                value: native.payload.clone(),
            }),
            "sweep" => {
                event.io.push(IoObservation {
                    port: "reaction_out".into(),
                    direction: IoDirection::Output,
                    channel: IoChannel::Extension("reaction_commit".into()),
                    value: native.payload.clone(),
                });
                event.io.push(IoObservation {
                    port: "matter_out".into(),
                    direction: IoDirection::Output,
                    channel: IoChannel::Extension("matter_ledger".into()),
                    value: native.payload.clone(),
                });
            }
            "catalyst" => {
                let target = native
                    .payload
                    .get("spark_targets")
                    .and_then(Value::as_array)
                    .filter(|targets| targets.len() == 1)
                    .and_then(|targets| targets[0].as_u64());
                if let (Some(from), Some(target)) = (&context.pc, target) {
                    event.control_flow = Some(ControlFlow {
                        kind: ControlFlowKind::Next,
                        from: Some(from.clone()),
                        to: Some(Address::new("well", target)),
                        taken: true,
                    });
                }
            }
            _ => {}
        }
        event
            .extensions
            .insert("hexwell.native".into(), native.payload.clone());
        Ok(vec![event])
    }
}

fn validate_context(context: &EventContext) -> Result<(), String> {
    if context.arch != MachineId::from(MACHINE_ID) {
        return Err(format!(
            "Hexwell adapter cannot normalize foreign architecture {:?}",
            context.arch
        ));
    }
    if context.machine_version != VersionStamp::from(MACHINE_VERSION) {
        return Err(format!(
            "Hexwell adapter cannot normalize machine version {:?}",
            context.machine_version
        ));
    }
    Ok(())
}

fn validate_payload(native: &NativeEvent) -> Result<(), String> {
    let kind = native.kind.as_str();
    match kind {
        "tide" => {
            let object = exact_object(&native.payload, &["mask", "ordinal"], kind)?;
            require_u8(object, "mask", kind)?;
            require_u64(object, "ordinal", kind)?;
        }
        "feed" => {
            let object = exact_object(
                &native.payload,
                &["introduced", "ignited", "changed_wells"],
                kind,
            )?;
            require_u64(object, "introduced", kind)?;
            require_u64(object, "ignited", kind)?;
            require_u64_array(object, "changed_wells", kind)?;
        }
        "frame" => {
            let object = exact_object(
                &native.payload,
                &["cooled_wells", "active_wells", "matter"],
                kind,
            )?;
            for field in ["cooled_wells", "active_wells", "matter"] {
                require_u64(object, field, kind)?;
            }
        }
        "quenched" => {
            exact_object(&native.payload, &[], kind)?;
        }
        "catalyst" => {
            let object = exact_object(
                &native.payload,
                &["family", "arg", "materia", "spark_targets", "commit"],
                kind,
            )?;
            require_string(object, "family", kind)?;
            require_u8(object, "arg", kind)?;
            require_string(object, "materia", kind)?;
            require_u64_array(object, "spark_targets", kind)?;
            if require_string(object, "commit", kind)? != "simultaneous_at_sweep_end" {
                return Err("Hexwell catalyst commit policy is unsupported".into());
            }
        }
        "sweep" => {
            let object = exact_object(
                &native.payload,
                &[
                    "sweep",
                    "active_before",
                    "active_after",
                    "matter_before",
                    "matter_after",
                    "precipitations",
                    "dissolutions",
                    "bindings",
                    "cleavages",
                    "tinctures",
                    "vented",
                    "transfers",
                ],
                kind,
            )?;
            for field in [
                "sweep",
                "active_before",
                "active_after",
                "matter_before",
                "matter_after",
                "precipitations",
                "dissolutions",
                "bindings",
                "cleavages",
                "tinctures",
                "vented",
            ] {
                require_u64(object, field, kind)?;
            }
            let transfers = object
                .get("transfers")
                .and_then(Value::as_array)
                .ok_or_else(|| "Hexwell sweep transfers must be an array".to_string())?;
            for transfer in transfers {
                let transfer = exact_object(
                    transfer,
                    &["from", "to", "materia", "requested", "accepted"],
                    "sweep transfer",
                )?;
                require_u64(transfer, "from", "sweep transfer")?;
                require_u64(transfer, "to", "sweep transfer")?;
                require_string(transfer, "materia", "sweep transfer")?;
                require_u64(transfer, "requested", "sweep transfer")?;
                require_u64(transfer, "accepted", "sweep transfer")?;
            }
        }
        other => return Err(format!("unsupported Hexwell native event kind {other:?}")),
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
        .ok_or_else(|| format!("Hexwell {kind} event payload must be an object"))?;
    if object.len() != fields.len() || fields.iter().any(|field| !object.contains_key(*field)) {
        return Err(format!(
            "Hexwell {kind} event payload must contain exactly {:?}",
            fields
        ));
    }
    Ok(object)
}

fn require_u8(object: &Map<String, Value>, field: &str, kind: &str) -> Result<u8, String> {
    let value = require_u64(object, field, kind)?;
    u8::try_from(value).map_err(|_| format!("Hexwell {kind} {field:?} exceeds 8 bits"))
}

fn require_u64(object: &Map<String, Value>, field: &str, kind: &str) -> Result<u64, String> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("Hexwell {kind} event requires unsigned {field:?}"))
}

fn require_u64_array(object: &Map<String, Value>, field: &str, kind: &str) -> Result<(), String> {
    let values = object
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("Hexwell {kind} event requires array {field:?}"))?;
    if values.iter().any(|value| value.as_u64().is_none()) {
        return Err(format!(
            "Hexwell {kind} event {field:?} must contain unsigned integers"
        ));
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
        .ok_or_else(|| format!("Hexwell {kind} event requires string {field:?}"))
}
