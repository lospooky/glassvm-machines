use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use glassvm_core::{
    ArtifactIdentity, ConfigField, ConfigFieldKind, EmulatorBackend, EmulatorSession,
    EventProjectionFingerprintAccumulator, ExecutionRequest, MachineConfigSchema, MachineId,
    PreparedConfiguration, PreparedObservation, PreparedRun, RunId, SchemaRef, SchemaVersion,
    StructuredValue,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::emulator_session::Chip8Session;
use crate::identity::CHIP8_ID;
use crate::session_snapshot::{SessionLifecycle, snapshot_to_bytes};
use crate::trace::observation_requires_step_records;

pub struct Chip8EmulatorBackend {
    pub(crate) live_inputs: Arc<Mutex<BTreeMap<RunId, crate::live_input::SharedChip8LiveInput>>>,
}

impl Chip8EmulatorBackend {
    pub fn new() -> Self {
        Self {
            live_inputs: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}

impl Default for Chip8EmulatorBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Explicit CUDA extension for the concrete CHIP-8 GlassVM bundle.
///
/// Keeping acceleration in an opt-in extension preserves `MachineBundle`'s
/// canonical scalar evidence contract while still making the GPU path
/// discoverable from the concrete bundle type. No method silently falls back
/// to the CPU.
#[cfg(feature = "cuda")]
pub trait Chip8CudaExt {
    fn cuda_batch_evaluator(
        &self,
        device_ordinal: usize,
    ) -> Result<chip8_core::CudaBatchEvaluator, chip8_core::CudaError>;

    fn cuda_batch_evaluator_with_options(
        &self,
        device_ordinal: usize,
        options: chip8_core::CudaBatchOptions,
    ) -> Result<chip8_core::CudaBatchEvaluator, chip8_core::CudaError>;
}

#[cfg(feature = "cuda")]
impl Chip8CudaExt for crate::Chip8Plugin {
    fn cuda_batch_evaluator(
        &self,
        device_ordinal: usize,
    ) -> Result<chip8_core::CudaBatchEvaluator, chip8_core::CudaError> {
        self.cuda_batch_evaluator(device_ordinal)
    }

    fn cuda_batch_evaluator_with_options(
        &self,
        device_ordinal: usize,
        options: chip8_core::CudaBatchOptions,
    ) -> Result<chip8_core::CudaBatchEvaluator, chip8_core::CudaError> {
        self.cuda_batch_evaluator_with_options(device_ordinal, options)
    }
}

impl Chip8EmulatorBackend {
    /// Construct the explicitly selected native CUDA batch evaluator.
    ///
    /// This is deliberately separate from the canonical prepared scalar
    /// GlassVM session: the CPU-backed session provides rich ordered traces,
    /// while CUDA returns its narrower typed batch result.
    /// Construction errors are returned directly and never trigger CPU fallback.
    #[cfg(feature = "cuda")]
    pub fn cuda_batch_evaluator(
        &self,
        device_ordinal: usize,
    ) -> Result<chip8_core::CudaBatchEvaluator, chip8_core::CudaError> {
        chip8_core::CudaBatchEvaluator::new(device_ordinal)
    }

    /// Construct the native CUDA batch evaluator with explicit chunking and
    /// launch controls.
    #[cfg(feature = "cuda")]
    pub fn cuda_batch_evaluator_with_options(
        &self,
        device_ordinal: usize,
        options: chip8_core::CudaBatchOptions,
    ) -> Result<chip8_core::CudaBatchEvaluator, chip8_core::CudaError> {
        chip8_core::CudaBatchEvaluator::with_options(device_ordinal, options)
    }

    fn create_concrete_execution_with_prepared_run(
        &self,
        rom_bytes: &[u8],
        request: ExecutionRequest,
        prepared_run: PreparedRun,
        prepared_observation: PreparedObservation,
    ) -> Result<Chip8Session, String> {
        request.validate_envelope()?;
        prepared_run.validate()?;
        prepared_observation.validate()?;
        if prepared_run.run_id != request.run_id {
            return Err("CHIP-8 prepared run ID does not match the execution request".into());
        }
        if prepared_run.machine_id != request.machine_id
            || request.machine_id != MachineId::from(CHIP8_ID)
        {
            return Err("CHIP-8 prepared run targets the wrong machine".into());
        }
        if request.prepared_observation_id != Some(prepared_observation.identity)
            || prepared_observation.request != request.observation.clone()
        {
            return Err("CHIP-8 prepared observation does not match the execution request".into());
        }

        let (variant, quirks, cycles_per_frame, machine_seed) =
            resolve_prepared_configuration(&prepared_run.configuration)?;
        let max_frames = prepared_run.execution_controls.frame_limit.ok_or_else(|| {
            "CHIP-8 publication execution requires an explicit frame_limit".to_string()
        })?;
        let scheduled_inputs =
            scheduled_inputs_to_key_inputs(&prepared_run.input_schedule, max_frames)?;
        let live_input_state = self.take_live_input_state(&prepared_run.run_id);

        let effective_seed = chip8_core::effective_seed(rom_bytes, machine_seed);
        let mut engine = chip8_core::Engine::new(rom_bytes, quirks, effective_seed)?;
        engine.set_cycles_per_frame(cycles_per_frame);
        let initial_snapshot = engine.save_snapshot();
        let initial_state_bytes = snapshot_to_bytes(&initial_snapshot);
        engine.set_step_recording(observation_requires_step_records(&request.observation));
        Ok(Chip8Session {
            request,
            engine,
            rom_bytes: rom_bytes.to_vec(),
            initial_snapshot,
            initial_state_bytes,
            artifact: ArtifactIdentity::from_bytes(rom_bytes),
            variant,
            effective_seed,
            max_frames,
            cycles_per_frame,
            scheduled_inputs: scheduled_inputs.clone(),
            next_scheduled_input: 0,
            cycles_into_frame: 0,
            event_projection_fingerprint: EventProjectionFingerprintAccumulator::new(),
            event_projection_accumulator_active: true,
            next_sequence: 0,
            lifecycle: SessionLifecycle::Fresh,
            prepared_observation,
            initial_scheduled_inputs: scheduled_inputs,
            live_input_state,
        })
    }
}

impl EmulatorBackend for Chip8EmulatorBackend {
    fn machine_id(&self) -> MachineId {
        MachineId(CHIP8_ID.to_string())
    }

    fn display_name(&self) -> String {
        "CHIP-8 / SCHIP / XO-CHIP".to_string()
    }

    fn config_schema(&self) -> MachineConfigSchema {
        MachineConfigSchema {
            schema: SchemaRef::new("chip8.machine_configuration", SchemaVersion::new(2, 0, 0)),
            fields: vec![
                ConfigField {
                    key: "quirks".to_string(),
                    label: "Quirks Preset".to_string(),
                    description: "Execution quirk preset used by the CHIP-8 core".to_string(),
                    kind: ConfigFieldKind::Enum {
                        options: vec![
                            "chip8".to_string(),
                            "chip48".to_string(),
                            "vip".to_string(),
                            "schip".to_string(),
                            "xochip".to_string(),
                        ],
                    },
                    default_value: Value::String("chip8".to_string()),
                },
                ConfigField {
                    key: "cycles_per_frame".into(),
                    label: "Cycles per frame".into(),
                    description: "Machine instructions executed per hardware frame".into(),
                    kind: ConfigFieldKind::Int {
                        min: 1,
                        max: i64::from(u32::MAX),
                        step: 1,
                    },
                    default_value: Value::from(12),
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
                    default_value: Value::from(0),
                },
            ],
        }
    }

    fn create_execution_with_prepared_run(
        &self,
        rom_bytes: &[u8],
        request: ExecutionRequest,
        prepared_run: PreparedRun,
        prepared_observation: glassvm_core::PreparedObservation,
    ) -> Result<Box<dyn EmulatorSession>, String> {
        Ok(Box::new(self.create_concrete_execution_with_prepared_run(
            rom_bytes,
            request,
            prepared_run,
            prepared_observation,
        )?))
    }
}

fn resolve_prepared_configuration(
    configuration: &PreparedConfiguration,
) -> Result<(String, chip8_core::QuirksConfig, u32, u64), String> {
    let values = match &configuration.value {
        StructuredValue::Map(values) => values,
        _ => return Err("CHIP-8 machine configuration must be a map".into()),
    };
    let quirks_name = match values.get("quirks") {
        Some(StructuredValue::Text(value)) => value.clone(),
        _ => return Err("CHIP-8 configuration field \"quirks\" must be text".into()),
    };
    let cycles = match values.get("cycles_per_frame") {
        Some(StructuredValue::Unsigned(value)) => u32::try_from(*value),
        Some(StructuredValue::Signed(value)) => u32::try_from(*value),
        _ => {
            return Err(
                "CHIP-8 configuration field \"cycles_per_frame\" must be a positive u32".into(),
            );
        }
    }
    .map_err(|_| {
        "CHIP-8 configuration field \"cycles_per_frame\" must be a positive u32".to_string()
    })?;
    let machine_seed = match values.get("machine_seed") {
        Some(StructuredValue::Unsigned(value)) => *value,
        Some(StructuredValue::Signed(value)) => u64::try_from(*value)
            .map_err(|_| "CHIP-8 machine_seed must be non-negative".to_string())?,
        _ => return Err("CHIP-8 configuration field \"machine_seed\" must be an integer".into()),
    };
    let (variant, quirks) = resolve_quirks(&BTreeMap::from([(
        String::from("quirks"),
        Value::String(quirks_name),
    )]))?;
    Ok((variant.into(), quirks, cycles, machine_seed))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledKeyInput {
    pub ordinal: u64,
    pub frame: u64,
    pub key: u8,
    pub active: bool,
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
                glassvm_core::InputCoordinate::Frame { frame } => frame,
                _ => {
                    return Err(format!(
                        "CHIP-8 input {} must use a frame coordinate",
                        entry.input_id
                    ));
                }
            };
            if frame >= max_frames {
                return Err(format!(
                    "CHIP-8 input {} targets frame {frame}, outside frame limit {max_frames}",
                    entry.input_id
                ));
            }
            let key = entry
                .input_id
                .as_str()
                .strip_prefix("chip8.key.")
                .and_then(|value| u8::from_str_radix(value, 16).ok())
                .filter(|key| *key < 16)
                .ok_or_else(|| format!("unsupported CHIP-8 input ID {}", entry.input_id))?;
            let active = match entry.payload.value {
                StructuredValue::Bool(active) => active,
                _ => {
                    return Err(format!(
                        "CHIP-8 input {} payload must be boolean",
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

#[doc(hidden)]
pub fn resolve_quirks(
    params: &BTreeMap<String, Value>,
) -> Result<(&'static str, chip8_core::QuirksConfig), String> {
    if let Some(unknown) = params.keys().find(|key| key.as_str() != "quirks") {
        return Err(format!("unknown CHIP-8 machine parameter {unknown:?}"));
    }
    let preset = match params.get("quirks") {
        None => "chip8",
        Some(Value::String(value)) => value.as_str(),
        Some(_) => return Err("CHIP-8 machine parameter \"quirks\" must be a string".into()),
    };
    match preset {
        "chip8" => Ok(("chip8", chip8_core::QuirksConfig::default())),
        // Refinery has historically exposed "chip48" as a distinct archive
        // stratum while the current native evaluator uses the modern/default
        // quirk set. Preserve that machine semantic until a future release
        // supplies its own parity evidence.
        "chip48" => Ok(("chip48", chip8_core::QuirksConfig::default())),
        "vip" => Ok(("vip", chip8_core::QuirksConfig::vip())),
        "schip" => Ok(("schip", chip8_core::QuirksConfig::schip())),
        "xochip" => Ok(("xochip", chip8_core::QuirksConfig::xochip())),
        _ => Err(format!(
            "unknown CHIP-8 quirks preset {preset:?}; expected chip8, chip48, vip, schip, or xochip"
        )),
    }
}
