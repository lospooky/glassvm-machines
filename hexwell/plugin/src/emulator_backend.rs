//! Hexwell: Emulator backend entry point and frozen-request validation.

use glassvm_core::{
    ConfigField, ConfigFieldKind, EmulatorBackend, EmulatorSession, ExecutionRequest,
    InputCoordinate, MachineConfigSchema, MachineId, PreparedConfiguration, PreparedObservation,
    PreparedRun, SchemaRef, SchemaVersion, StructuredValue,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::emulator_session::HexwellSession;
use crate::identity::{MACHINE_ID, SESSION_SNAPSHOT_FORMAT_VERSION};
use crate::session_snapshot::{ContinuationContract, SessionImage, SessionLifecycle};
use hexwell_core::{ReactorState, Telemetry};

pub struct HexwellEmulatorBackend;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ScheduledTideInput {
    pub ordinal: u64,
    pub frame: u64,
    pub value: u8,
}

impl HexwellEmulatorBackend {
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
            return Err("Hexwell prepared run ID does not match the execution request".into());
        }
        if prepared_run.machine_id != request.machine_id
            || request.machine_id != MachineId::from(MACHINE_ID)
        {
            return Err("Hexwell prepared run targets the wrong machine".into());
        }
        if request.prepared_observation_id != Some(prepared_observation.identity)
            || prepared_observation.request != request.observation.clone()
        {
            return Err("Hexwell prepared observation does not match the execution request".into());
        }

        let (sweeps_per_frame, machine_seed) =
            resolve_prepared_configuration(&prepared_run.configuration)?;
        let max_frames = prepared_run.execution_controls.frame_limit.ok_or_else(|| {
            "Hexwell publication execution requires an explicit frame_limit".to_string()
        })?;
        let scheduled_inputs =
            scheduled_inputs_to_tide_inputs(&prepared_run.input_schedule, max_frames)?;
        let reactor = ReactorState::boot(rom_bytes, machine_seed)?;
        let initial_matter = reactor.total_matter();
        let mut image = SessionImage {
            snapshot_version: SESSION_SNAPSHOT_FORMAT_VERSION,
            contract: ContinuationContract::from_request(&request),
            lifecycle: SessionLifecycle::Fresh,
            reactor,
            telemetry: Telemetry::boot(initial_matter),
            next_scheduled_input: 0,
            frame_open: false,
            sweeps_into_frame: 0,
            payload_digest: glassvm_core::ContentDigest::sha256(&[]),
        };
        image.refresh_digest()?;
        Ok(Box::new(HexwellSession {
            initial: image.clone(),
            image,
            request,
            started: false,
            failed: false,
            prepared_observation,
            max_frames,
            sweeps_per_frame,
            scheduled_inputs: scheduled_inputs.clone(),
            initial_scheduled_inputs: scheduled_inputs,
        }))
    }
}

impl EmulatorBackend for HexwellEmulatorBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(MACHINE_ID)
    }

    fn display_name(&self) -> String {
        "Hexwell Catalyst Plate".into()
    }

    fn config_schema(&self) -> MachineConfigSchema {
        MachineConfigSchema {
            schema: SchemaRef::new("hexwell.machine_configuration", SchemaVersion::V1),
            fields: vec![
                ConfigField {
                    key: "lattice_topology".into(),
                    label: "Lattice Topology".into(),
                    description:
                        "Fixed v1 odd-row offset hex torus; exposed to make replay semantics explicit"
                            .into(),
                    kind: ConfigFieldKind::Enum {
                        options: vec!["odd-r-hex-torus-v1".into()],
                    },
                    default_value: json!("odd-r-hex-torus-v1"),
                },
                ConfigField {
                    key: "cooling_per_frame".into(),
                    label: "Cooling per Frame".into(),
                    description:
                        "Fixed v1 passive heat loss applied after the final sweep of each frame"
                            .into(),
                    kind: ConfigFieldKind::Int {
                        min: 1,
                        max: 1,
                        step: 1,
                    },
                    default_value: json!(1),
                },
                ConfigField {
                    key: "sweeps_per_frame".into(),
                    label: "Sweeps per frame".into(),
                    description: "Reaction sweeps grouped into one work frame".into(),
                    kind: ConfigFieldKind::Int {
                        min: 1,
                        max: i64::from(u32::MAX),
                        step: 1,
                    },
                    default_value: json!(2),
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
        _ => return Err("Hexwell machine configuration must be a map".into()),
    };
    let sweeps = match values.get("sweeps_per_frame") {
        Some(StructuredValue::Unsigned(value)) => u32::try_from(*value),
        Some(StructuredValue::Signed(value)) => u32::try_from(*value),
        _ => {
            return Err(
                "Hexwell configuration field \"sweeps_per_frame\" must be a positive u32".into(),
            );
        }
    }
    .map_err(|_| {
        "Hexwell configuration field \"sweeps_per_frame\" must be a positive u32".to_string()
    })?;
    let machine_seed = match values.get("machine_seed") {
        Some(StructuredValue::Unsigned(value)) => *value,
        Some(StructuredValue::Signed(value)) => u64::try_from(*value)
            .map_err(|_| "Hexwell machine_seed must be non-negative".to_string())?,
        _ => return Err("Hexwell machine_seed must be an integer".into()),
    };
    Ok((sweeps, machine_seed))
}

fn scheduled_inputs_to_tide_inputs(
    schedule: &glassvm_core::ResolvedInputSchedule,
    max_frames: u64,
) -> Result<Vec<ScheduledTideInput>, String> {
    schedule
        .entries
        .iter()
        .map(|entry| {
            let frame = match entry.coordinate {
                InputCoordinate::Frame { frame } => frame,
                _ => {
                    return Err(format!(
                        "Hexwell input {} must use a frame coordinate",
                        entry.input_id
                    ));
                }
            };
            if frame >= max_frames {
                return Err(format!(
                    "Hexwell input {} targets frame {frame}, outside frame limit {max_frames}",
                    entry.input_id
                ));
            }
            if entry.input_id.as_str() != "hexwell.tide" {
                return Err(format!("unsupported Hexwell input ID {}", entry.input_id));
            }
            let value = match &entry.payload.value {
                StructuredValue::Unsigned(value) => u8::try_from(*value).map_err(|_| ()),
                StructuredValue::Signed(value) => u8::try_from(*value).map_err(|_| ()),
                _ => Err(()),
            }
            .map_err(|_| {
                format!(
                    "Hexwell input {} payload must be an unsigned eight-bit value",
                    entry.input_id
                )
            })?;
            Ok(ScheduledTideInput {
                ordinal: entry.ordinal,
                frame,
                value,
            })
        })
        .collect()
}
