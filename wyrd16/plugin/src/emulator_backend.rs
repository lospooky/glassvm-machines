//! GlassVM emulator-backend entry point.

use glassvm_core::{
    ConfigField, ConfigFieldKind, EmulatorBackend, EmulatorSession, ExecutionRequest,
    InputCoordinate, MachineConfigSchema, MachineId, PreparedConfiguration, PreparedObservation,
    PreparedRun, SchemaRef, SchemaVersion, StructuredValue,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{emulator_session::Wyrd16Session, identity::MACHINE_ID};

pub struct Wyrd16EmulatorBackend;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ScheduledKeyInput {
    pub ordinal: u64,
    pub frame: u64,
    pub key: u8,
    pub active: bool,
}

impl Wyrd16EmulatorBackend {
    fn create_execution_with_prepared_run_internal(
        &self,
        rom_bytes: &[u8],
        request: ExecutionRequest,
        prepared_run: PreparedRun,
        prepared_observation: PreparedObservation,
    ) -> Result<Box<dyn EmulatorSession>, String> {
        request.validate_envelope()?;
        prepared_run.validate()?;
        prepared_observation.validate()?;
        if prepared_run.run_id != request.run_id {
            return Err("Wyrd-16 prepared run ID does not match the execution request".into());
        }
        if prepared_run.machine_id != request.machine_id
            || request.machine_id != MachineId::from(MACHINE_ID)
        {
            return Err("Wyrd-16 prepared run targets the wrong machine".into());
        }
        if request.prepared_observation_id != Some(prepared_observation.identity)
            || prepared_observation.request != request.observation.clone()
        {
            return Err("Wyrd-16 prepared observation does not match the execution request".into());
        }

        let (cycles_per_frame, machine_seed) =
            resolve_prepared_configuration(&prepared_run.configuration)?;
        let max_frames = prepared_run.execution_controls.frame_limit.ok_or_else(|| {
            "Wyrd-16 publication execution requires an explicit frame_limit".to_string()
        })?;
        let scheduled_inputs =
            scheduled_inputs_to_key_inputs(&prepared_run.input_schedule, max_frames)?;
        Ok(Box::new(Wyrd16Session::new(
            rom_bytes,
            request,
            prepared_observation,
            max_frames,
            cycles_per_frame,
            machine_seed,
            scheduled_inputs,
        )?))
    }
}

impl EmulatorBackend for Wyrd16EmulatorBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(MACHINE_ID)
    }

    fn display_name(&self) -> String {
        "Wyrd-16 Rune Computer".into()
    }

    fn config_schema(&self) -> MachineConfigSchema {
        MachineConfigSchema {
            schema: SchemaRef::new("wyrd16.machine_configuration", SchemaVersion::V1),
            fields: vec![
                ConfigField {
                    key: "canvas_decay".into(),
                    label: "Canvas Decay".into(),
                    description: "Reserved v1 parameter; must remain zero for native semantics"
                        .into(),
                    kind: ConfigFieldKind::Int {
                        min: 0,
                        max: 0,
                        step: 1,
                    },
                    default_value: json!(0),
                },
                ConfigField {
                    key: "rune_cycles_per_frame".into(),
                    label: "Rune cycles per frame".into(),
                    description: "Rune actions grouped into one work frame".into(),
                    kind: ConfigFieldKind::Int {
                        min: 1,
                        max: i64::from(u32::MAX),
                        step: 1,
                    },
                    default_value: json!(8),
                },
                ConfigField {
                    key: "machine_seed".into(),
                    label: "Machine seed".into(),
                    description: "Seed governing machine state evolution".into(),
                    kind: ConfigFieldKind::Int {
                        min: 0,
                        max: i64::MAX,
                        step: 1,
                    },
                    default_value: json!(0),
                },
            ],
        }
    }

    fn create_execution_with_prepared_run(
        &self,
        rom_bytes: &[u8],
        request: ExecutionRequest,
        prepared_run: PreparedRun,
        prepared_observation: PreparedObservation,
    ) -> Result<Box<dyn EmulatorSession>, String> {
        self.create_execution_with_prepared_run_internal(
            rom_bytes,
            request,
            prepared_run,
            prepared_observation,
        )
    }
}

fn resolve_prepared_configuration(
    configuration: &PreparedConfiguration,
) -> Result<(u32, u64), String> {
    let values = match &configuration.value {
        StructuredValue::Map(values) => values,
        _ => return Err("Wyrd-16 machine configuration must be a map".into()),
    };
    let cycles = match values.get("rune_cycles_per_frame") {
        Some(StructuredValue::Unsigned(value)) => u32::try_from(*value),
        Some(StructuredValue::Signed(value)) => u32::try_from(*value),
        _ => {
            return Err(
                "Wyrd-16 configuration field \"rune_cycles_per_frame\" must be a positive u32"
                    .into(),
            );
        }
    }
    .map_err(|_| {
        "Wyrd-16 configuration field \"rune_cycles_per_frame\" must be a positive u32".to_string()
    })?;
    let machine_seed = match values.get("machine_seed") {
        Some(StructuredValue::Unsigned(value)) => *value,
        Some(StructuredValue::Signed(value)) => u64::try_from(*value)
            .map_err(|_| "Wyrd-16 machine_seed must be non-negative".to_string())?,
        _ => return Err("Wyrd-16 machine_seed must be an integer".into()),
    };
    Ok((cycles, machine_seed))
}

fn scheduled_inputs_to_key_inputs(
    schedule: &glassvm_core::ResolvedInputSchedule,
    max_frames: u64,
) -> Result<Vec<ScheduledKeyInput>, String> {
    schedule
        .entries
        .iter()
        .map(|entry| {
            let frame = match entry.coordinate {
                InputCoordinate::Frame { frame } => frame,
                _ => {
                    return Err(format!(
                        "Wyrd-16 input {} must use a frame coordinate",
                        entry.input_id
                    ));
                }
            };
            if frame >= max_frames {
                return Err(format!(
                    "Wyrd-16 input {} targets frame {frame}, outside frame limit {max_frames}",
                    entry.input_id
                ));
            }
            let key = entry
                .input_id
                .as_str()
                .strip_prefix("wyrd16.key.")
                .and_then(|value| value.parse::<u8>().ok())
                .filter(|key| *key < 8)
                .ok_or_else(|| format!("unsupported Wyrd-16 input ID {}", entry.input_id))?;
            let active = match &entry.payload.value {
                StructuredValue::Bool(active) => *active,
                _ => {
                    return Err(format!(
                        "Wyrd-16 input {} payload must be boolean",
                        entry.input_id
                    ));
                }
            };
            Ok(ScheduledKeyInput {
                ordinal: entry.ordinal,
                frame,
                key,
                active,
            })
        })
        .collect()
}
