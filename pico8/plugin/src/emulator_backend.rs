use std::collections::BTreeMap;

use glassvm_core::{
    ConfigField, ConfigFieldKind, EmissionSink, EmulatorBackend, EmulatorSession, ExecutionRequest,
    MachineConfigSchema, MachineId, ReplayPackage, ResolvedEpisodeContext, RunConfig, RunId,
    TraceFingerprintAccumulator,
};
use pico8_core::{Cartridge, Pico8Runtime};
use serde_json::{Value, json};

use crate::emulator_session::{Pico8Session, validate_stimuli};
use crate::identity::{DEFAULT_INSTRUCTION_BUDGET, PICO8_ID};
use crate::replay::{Pico8ReplayOutcome, ReceiptTap, resolve_episode_context};
use crate::session_snapshot::{SessionLifecycle, observable_state_bytes};

pub struct Pico8EmulatorBackend;

impl Pico8EmulatorBackend {
    fn create_concrete_execution(
        &self,
        artifact: &[u8],
        mut request: ExecutionRequest,
    ) -> Result<Pico8Session, String> {
        request.validate_envelope()?;
        if request.machine_id.as_str() != PICO8_ID {
            return Err(format!(
                "PICO-8 backend cannot execute machine {:?}",
                request.machine_id
            ));
        }
        validate_machine_params(&request.config.machine_params)?;
        validate_stimuli(&request.stimuli, request.config.max_frames)?;
        request.episode = Some(resolve_episode_context(&request)?);
        let cartridge = Cartridge::parse(artifact)
            .map_err(|error| format!("invalid PICO-8 cartridge: {error}"))?;
        let instruction_budget = resolve_instruction_budget(&request)?;
        let runtime = Pico8Runtime::new(&cartridge, request.config.seed, instruction_budget)?;
        let initial_state_bytes = observable_state_bytes(&runtime.snapshot())?;
        let stimuli = request.stimuli.clone();
        Ok(Pico8Session {
            request,
            cartridge,
            artifact_bytes: artifact.to_vec(),
            runtime,
            instruction_budget,
            initial_state_bytes,
            stimuli,
            next_stimulus: 0,
            next_sequence: 0,
            trace_fingerprint: TraceFingerprintAccumulator::new(),
            pending_trace_events: Vec::new(),
            next_trace_chunk: 0,
            previous_trace_chunk_digest: None,
            frame_hashes: Vec::new(),
            lifecycle: SessionLifecycle::Fresh,
            runtime_error: None,
        })
    }

    pub fn replay_package(
        &self,
        artifact: &[u8],
        package: &ReplayPackage,
        run_id: impl Into<RunId>,
        sink: &mut dyn EmissionSink,
    ) -> Result<Pico8ReplayOutcome, String> {
        let mut preflight = package.validate_internal();
        preflight.extend(package.manifest.compare_artifact(artifact));
        if !preflight.is_match() {
            return Ok(Pico8ReplayOutcome {
                preflight,
                result: None,
                receipt: None,
                outcome: None,
            });
        }
        let max_frames = package
            .manifest
            .budget
            .max_frames
            .ok_or_else(|| "PICO-8 replay manifest lacks max_frames".to_string())?;
        let instruction_budget = package
            .manifest
            .machine
            .machine_parameters
            .get("instruction_budget")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                "PICO-8 replay manifest lacks a numeric instruction_budget".to_string()
            })?;
        let episode = ResolvedEpisodeContext::new(
            package.manifest.input_policy.clone(),
            package.manifest.body.clone(),
            package.manifest.environment.clone(),
            package.manifest.seeds.policy_seed,
            package.manifest.seeds.environment_seed,
        )?;
        let request = ExecutionRequest::new(
            run_id,
            PICO8_ID,
            RunConfig {
                max_frames,
                cycles_per_frame: 1,
                seed: package.manifest.seeds.requested,
                machine_params: BTreeMap::from([(
                    "instruction_budget".into(),
                    json!(instruction_budget),
                )]),
                record_frames: false,
                record_events: false,
            },
            package.manifest.observation.clone(),
        )?
        .with_stimuli(package.stimuli.clone())?
        .with_resolved_episode(episode)?;
        let mut session = self.create_concrete_execution(artifact, request)?;
        let actual_manifest = session.replay_manifest()?;
        preflight.extend(package.manifest.compare(&actual_manifest));
        if !preflight.is_match() {
            return Ok(Pico8ReplayOutcome {
                preflight,
                result: None,
                receipt: None,
                outcome: None,
            });
        }
        let mut tap = ReceiptTap {
            downstream: sink,
            receipt: None,
        };
        let result = session.execute(&mut tap)?;
        let receipt = tap
            .receipt
            .ok_or_else(|| "PICO-8 replay emitted no receipt".to_string())?;
        let outcome = package.compare_receipt(&receipt);
        Ok(Pico8ReplayOutcome {
            preflight,
            result: Some(result),
            receipt: Some(receipt),
            outcome: Some(outcome),
        })
    }
}

impl EmulatorBackend for Pico8EmulatorBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(PICO8_ID)
    }

    fn display_name(&self) -> String {
        "PICO-8 (headless compatibility runtime)".into()
    }

    fn config_schema(&self) -> MachineConfigSchema {
        MachineConfigSchema {
            fields: vec![ConfigField {
                key: "instruction_budget".into(),
                label: "Lua instructions per frame".into(),
                description:
                    "Deterministic safety ceiling; execution traps when a callback frame exceeds it"
                        .into(),
                kind: ConfigFieldKind::Int {
                    min: 1_000,
                    max: 50_000_000,
                    step: 1_000,
                },
                default_value: json!(DEFAULT_INSTRUCTION_BUDGET),
            }],
        }
    }

    fn create_execution(
        &self,
        artifact: &[u8],
        request: ExecutionRequest,
    ) -> Result<Box<dyn EmulatorSession>, String> {
        Ok(Box::new(self.create_concrete_execution(artifact, request)?))
    }
}

pub(super) fn resolve_instruction_budget(request: &ExecutionRequest) -> Result<u64, String> {
    if request.config.cycles_per_frame == 0 {
        return Err("PICO-8 cycles_per_frame must be non-zero".into());
    }
    let derived = u64::from(request.config.cycles_per_frame) * 1_000;
    let value = request
        .config
        .machine_params
        .get("instruction_budget")
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| "PICO-8 instruction_budget must be an unsigned integer".to_string())
        })
        .transpose()?
        .unwrap_or(derived);
    if !(1_000..=50_000_000).contains(&value) {
        return Err(format!(
            "PICO-8 instruction_budget {value} is outside 1000..=50000000"
        ));
    }
    Ok(value)
}

pub(super) fn validate_machine_params(params: &BTreeMap<String, Value>) -> Result<(), String> {
    for key in params.keys() {
        if key != "instruction_budget" {
            return Err(format!("unsupported PICO-8 machine parameter {key:?}"));
        }
    }
    Ok(())
}
