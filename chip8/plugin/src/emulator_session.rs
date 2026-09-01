use glassvm_core::{
    Address, ArtifactIdentity, CausalLink, CausalRelation, CommonMetrics, ContentDigest, Emission,
    EmissionSink, EmulatorSession, EventContext, EventKind, EventProjectionFingerprintAccumulator,
    ExecutionEvent, ExecutionRequest, FrameArtifact, FrameCapture, InputApplied, InputCoordinate,
    InputId, InputSource, InputValueEvidence, IoChannel, IoDirection, IoObservation, MachineId,
    NativeEvidenceEnvelope, NormalizerDriver, ObservationRequest, PreparedObservation, RunResult,
    SnapshotArtifact, SnapshotCapture, StateSpace, StructuredValue, TypedInputPayload,
    VersionStamp, canonical_json_fingerprint,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::adapters::Chip8NativeEventAdapter;
use crate::contract::interestingness_schema;
use crate::emulator_backend::ScheduledKeyInput;
use crate::identity::{
    CHIP8_ID, EMULATOR_VERSION, SEMANTICS, SESSION_SNAPSHOT_FORMAT,
    SESSION_SNAPSHOT_FORMAT_VERSION, schema, session_snapshot_schema,
};
use crate::session_snapshot::{
    SESSION_SNAPSHOT_PAYLOAD_DOMAIN, SessionLifecycle, snapshot_from_bytes, snapshot_to_bytes,
};
use crate::trace::{
    causal_relation_for, chip8_instruction_reads, chip8_instruction_ref, conditional_branch_event,
    enrich_normalized_event, fatal_step_trap, key_mask_location, named_location,
    native_event_from_step, observation_requires_step_records, observed_read, observed_write,
    randomness_sample_event, sink_error, state_delta_events, termination_from_step_result,
    termination_to_string, timer_location, waiting_input_sample_event,
};

#[doc(hidden)]
pub struct Chip8Session {
    pub(super) request: ExecutionRequest,
    pub engine: chip8_core::Engine,
    pub(super) rom_bytes: Vec<u8>,
    pub(super) initial_snapshot: chip8_core::Snapshot,
    pub(super) initial_state_bytes: Vec<u8>,
    pub(super) artifact: ArtifactIdentity,
    pub(super) variant: String,
    pub(super) effective_seed: u64,
    pub(super) max_frames: u64,
    pub(super) max_steps: Option<u64>,
    pub(super) cycles_per_frame: u32,
    pub(super) scheduled_inputs: Vec<ScheduledKeyInput>,
    pub(super) next_scheduled_input: usize,
    pub(super) cycles_into_frame: u32,
    pub(super) event_projection_fingerprint: EventProjectionFingerprintAccumulator,
    pub(super) event_projection_accumulator_active: bool,
    pub(super) next_sequence: u64,
    pub(super) lifecycle: SessionLifecycle,
    pub(super) prepared_observation: PreparedObservation,
    pub(super) initial_scheduled_inputs: Vec<ScheduledKeyInput>,
    pub(super) live_input_state: Option<crate::live_input::SharedChip8LiveInput>,
}

enum Chip8ExecutionSink<'a> {
    Direct(&'a mut dyn EmissionSink),
}

impl EmissionSink for Chip8ExecutionSink<'_> {
    fn emit(&mut self, emission: Emission<'_>) -> Result<(), glassvm_core::SinkError> {
        match self {
            Self::Direct(sink) => sink.emit(emission),
        }
    }

    fn record_capability_receipts(&mut self, receipts: &[glassvm_core::CapabilityReceipt]) {
        match self {
            Self::Direct(sink) => sink.record_capability_receipts(receipts),
        }
    }
}

impl Chip8ExecutionSink<'_> {
    fn finish(self) -> Result<(), String> {
        match self {
            Self::Direct(_) => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[doc(hidden)]
pub struct Chip8SessionSnapshotPayload {
    pub(super) request_digest: ContentDigest,
    pub(super) artifact: ArtifactIdentity,
    pub(super) bundle_version: VersionStamp,
    pub(super) machine_version: VersionStamp,
    pub(super) emulator_version: VersionStamp,
    pub(super) variant: String,
    pub(super) effective_seed: u64,
    pub(super) initial_state_digest: ContentDigest,
    pub engine_state: Vec<u8>,
    pub(super) engine_cycle_count: u64,
    pub engine_frame_count: u64,
    pub cycles_into_frame: u32,
    pub engine_evidence: ContentDigest,
    pub scheduled_inputs: Vec<ScheduledKeyInput>,
    pub next_scheduled_input: usize,
    pub(super) event_projection_accumulator_active: bool,
    pub next_sequence: u64,
    pub(super) lifecycle: SessionLifecycle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[doc(hidden)]
pub struct Chip8SessionSnapshot {
    pub(super) format: String,
    pub(super) version: u32,
    pub payload: Chip8SessionSnapshotPayload,
    pub payload_digest: ContentDigest,
}

impl EmulatorSession for Chip8Session {
    fn request(&self) -> &ExecutionRequest {
        &self.request
    }

    fn execute(&mut self, sink: &mut dyn EmissionSink) -> Result<RunResult, String> {
        let execution_sink = Chip8ExecutionSink::Direct(sink);
        let mut normalizer_driver = NormalizerDriver::new_with_prepared_observation(
            crate::normalizer::Chip8Normalizer::new(),
            execution_sink,
            &self.prepared_observation,
        );
        let sink: &mut dyn EmissionSink = &mut normalizer_driver;
        match self.lifecycle {
            SessionLifecycle::Fresh => {
                // Poison the session before the first fallible operation. A
                // rejected sink or malformed internal state cannot be retried
                // without an explicit reset.
                self.lifecycle = SessionLifecycle::Terminal;
            }
            SessionLifecycle::Incremental => {
                self.lifecycle = SessionLifecycle::Terminal;
                return Err(
                    "CHIP-8 session cannot execute after stepping, input, or snapshot restore; reset first"
                        .into(),
                );
            }
            SessionLifecycle::Terminal => {
                return Err("CHIP-8 execute() may only be attempted once; reset first".into());
            }
        }
        if self.engine.cycle_count() != 0
            || self.engine.frame_count() != 0
            || snapshot_to_bytes(&self.engine.save_snapshot()) != self.initial_state_bytes
        {
            return Err(
                "replayable CHIP-8 execution must begin at the authoritative boot state; reset the session after stepping, restoring, or changing input"
                    .into(),
            );
        }
        self.next_sequence = 0;
        self.next_scheduled_input = 0;
        self.cycles_into_frame = 0;
        self.event_projection_fingerprint = EventProjectionFingerprintAccumulator::new();
        self.event_projection_accumulator_active = true;
        let mut final_state = None;

        sink.emit(Emission::RunStarted(&self.request))
            .map_err(sink_error)?;
        self.emit_lifecycle_event(EventKind::RunStarted, sink)?;

        let mut termination = chip8_core::TerminationReason::Timeout;
        for frame_index in 0..self.max_frames {
            if self.step_limit_reached() {
                break;
            }
            self.apply_scheduled_inputs_for_current_frame(sink)?;
            let mut terminal_periodic_snapshot = false;
            let frame_count_before = self.engine.frame_count();
            let frame_result = if self.engine.step_recording() || self.max_steps.is_some() {
                let mut result = chip8_core::FrameResult::Ok;
                let steps_this_frame = self.steps_available_in_frame();
                for _ in 0..steps_this_frame {
                    let step_result = self.engine.step_in_frame();
                    self.cycles_into_frame = self
                        .cycles_into_frame
                        .checked_add(1)
                        .ok_or_else(|| "CHIP-8 partial-frame counter overflow".to_string())?;
                    self.emit_pending_step_records(sink)?;
                    let snapshot_lifecycle = if matches!(
                        &step_result,
                        chip8_core::StepResult::Ok | chip8_core::StepResult::WaitingForKey
                    ) {
                        SessionLifecycle::Incremental
                    } else {
                        SessionLifecycle::Terminal
                    };
                    let reaches_final_frame_boundary = snapshot_lifecycle
                        == SessionLifecycle::Incremental
                        && self.cycles_into_frame == self.engine.cycles_per_frame()
                        && frame_index.saturating_add(1) >= self.max_frames;
                    if reaches_final_frame_boundary {
                        terminal_periodic_snapshot = true;
                    } else {
                        self.emit_periodic_snapshot(snapshot_lifecycle, sink)?;
                    }
                    match step_result {
                        chip8_core::StepResult::Ok | chip8_core::StepResult::WaitingForKey => {}
                        chip8_core::StepResult::Halted => {
                            result = chip8_core::FrameResult::Halted;
                            break;
                        }
                        error => {
                            result = chip8_core::FrameResult::Error(error);
                            break;
                        }
                    }
                }
                if matches!(result, chip8_core::FrameResult::Ok)
                    && self.cycles_into_frame == self.engine.cycles_per_frame()
                {
                    self.engine.finish_frame();
                    self.cycles_into_frame = 0;
                }
                result
            } else {
                let before = self.engine.cycle_count();
                let result = self.engine.step_frame();
                let advanced = self.engine.cycle_count().saturating_sub(before);
                if matches!(result, chip8_core::FrameResult::Ok) {
                    self.cycles_into_frame = 0;
                } else {
                    self.cycles_into_frame =
                        self.cycles_into_frame
                            .checked_add(u32::try_from(advanced).map_err(|_| {
                                "CHIP-8 partial-frame progress exceeds u32".to_string()
                            })?)
                            .ok_or_else(|| "CHIP-8 partial-frame counter overflow".to_string())?;
                }
                result
            };
            match frame_result {
                chip8_core::FrameResult::Ok => {}
                chip8_core::FrameResult::Halted => {
                    termination = chip8_core::TerminationReason::Completed;
                    self.emit_pending_step_records(sink)?;
                    break;
                }
                chip8_core::FrameResult::Error(error) => {
                    termination = termination_from_step_result(&error);
                    self.emit_pending_step_records(sink)?;
                    break;
                }
            }
            if self.step_limit_reached() && self.engine.frame_count() == frame_count_before {
                break;
            }
            self.emit_pending_step_records(sink)?;
            self.emit_pending_frame_timer_records(sink)?;
            self.emit_frame_completed(sink)?;
            if terminal_periodic_snapshot {
                final_state = self.emit_periodic_snapshot(SessionLifecycle::Terminal, sink)?;
            }
        }

        let step_limit_reached = self.step_limit_reached();
        if !step_limit_reached
            && matches!(termination, chip8_core::TerminationReason::Timeout)
            && !matches!(self.engine.cpu.key_wait, chip8_core::KeyWait::None)
        {
            termination = chip8_core::TerminationReason::WaitingForInput;
        }

        let terminal_kind = if matches!(
            termination,
            chip8_core::TerminationReason::InvalidOpcode
                | chip8_core::TerminationReason::StackUnderflow
                | chip8_core::TerminationReason::StackOverflow
                | chip8_core::TerminationReason::MemoryFault
        ) {
            EventKind::RunCrashed
        } else {
            EventKind::RunHalted
        };
        match self.request.observation.snapshots.capture {
            SnapshotCapture::Final => {
                final_state = Some(self.emit_snapshot(SessionLifecycle::Terminal, sink)?);
            }
            SnapshotCapture::None | SnapshotCapture::EverySteps(_) => {}
        }
        self.emit_lifecycle_event(terminal_kind, sink)?;

        let mut result = Self::build_execution_result(&self.engine, termination.clone());
        if step_limit_reached {
            result.common.termination = "step_limit".into();
        }
        if !self
            .prepared_observation
            .native_observation_schemas
            .is_empty()
        {
            for observation in Self::native_evidence_for_engine(
                &self.engine,
                &self.request.run_id,
                termination.clone(),
            )? {
                sink.emit(Emission::NativeEvidence(&observation))
                    .map_err(sink_error)?;
            }
        }
        let normalizer_run = normalizer_driver
            .finish(final_state.as_ref())
            .map_err(|error| error.to_string())?;
        let (mut execution_sink, normalizer_output) = normalizer_run.into_parts();
        let sink: &mut dyn EmissionSink = &mut execution_sink;
        result.capabilities = normalizer_output.capabilities;
        let _event_projection = std::mem::take(&mut self.event_projection_fingerprint).finish();
        self.event_projection_accumulator_active = false;
        sink.emit(Emission::RunFinished(&result))
            .map_err(sink_error)?;
        execution_sink.finish()?;
        Ok(result)
    }

    fn step_frame(&mut self) -> Result<(), String> {
        if self.lifecycle == SessionLifecycle::Terminal {
            return Err("CHIP-8 session is terminal; reset before stepping".into());
        }
        if self.engine.frame_count() >= self.max_frames {
            return Err(format!(
                "CHIP-8 session reached frozen frame budget {}",
                self.max_frames
            ));
        }
        self.lifecycle = SessionLifecycle::Incremental;
        let result = (|| {
            if self.step_limit_reached() {
                return Ok(true);
            }
            if self.cycles_into_frame == 0 {
                self.apply_scheduled_inputs_for_current_frame(&mut glassvm_core::NullSink)?;
            }
            let remaining = self
                .engine
                .cycles_per_frame()
                .checked_sub(self.cycles_into_frame)
                .ok_or_else(|| "CHIP-8 partial-frame phase exceeds cycles_per_frame".to_string())?;
            let remaining = remaining.min(self.steps_available_in_frame());
            for _ in 0..remaining {
                let result = self.engine.step_in_frame();
                self.cycles_into_frame = self
                    .cycles_into_frame
                    .checked_add(1)
                    .ok_or_else(|| "CHIP-8 partial-frame counter overflow".to_string())?;
                match result {
                    chip8_core::StepResult::Ok | chip8_core::StepResult::WaitingForKey => {}
                    chip8_core::StepResult::Halted => return Ok(true),
                    err => return Err(format!("step_frame error: {err:?}")),
                }
            }
            if self.cycles_into_frame == self.engine.cycles_per_frame() {
                self.engine.finish_frame();
                self.cycles_into_frame = 0;
            }
            Ok(self.engine.frame_count() >= self.max_frames || self.step_limit_reached())
        })();
        match result {
            Ok(terminal) => {
                if terminal {
                    self.lifecycle = SessionLifecycle::Terminal;
                }
                Ok(())
            }
            Err(error) => {
                self.lifecycle = SessionLifecycle::Terminal;
                Err(error)
            }
        }
    }

    fn reset(&mut self) -> Result<(), String> {
        self.engine.load_snapshot(&self.initial_snapshot);
        if let Some(state) = &self.live_input_state {
            let mut state = state
                .lock()
                .map_err(|_| "CHIP-8 live-input state is unavailable".to_string())?;
            state.key_mask = 0;
            state.current_frame = 0;
        }
        self.engine
            .set_step_recording(observation_requires_step_records(&self.request.observation));
        self.scheduled_inputs = self.initial_scheduled_inputs.clone();
        self.next_scheduled_input = 0;
        self.cycles_into_frame = 0;
        self.event_projection_fingerprint = EventProjectionFingerprintAccumulator::new();
        self.event_projection_accumulator_active = true;
        self.next_sequence = 0;
        self.lifecycle = SessionLifecycle::Fresh;
        Ok(())
    }

    fn snapshot(&self) -> Result<Vec<u8>, String> {
        self.snapshot_bytes_for(self.lifecycle)
    }

    fn restore_snapshot(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.validate_fresh_restore_target()?;
        let snapshot: Chip8SessionSnapshot = match serde_json::from_slice(bytes) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                if snapshot_from_bytes(bytes).is_ok() {
                    return Err(
                        "legacy CHIP-8 CPU-only snapshots are incompatible with exact session restore"
                            .into(),
                    );
                }
                return Err(format!("invalid CHIP-8 session snapshot: {error}"));
            }
        };
        self.validate_snapshot_identity(&snapshot)?;
        self.validate_snapshot_session_state(&snapshot.payload)?;
        let engine = self.reconstruct_snapshot_engine(&snapshot.payload)?;

        let payload = snapshot.payload;
        self.engine = *engine;
        if let Some(state) = &self.live_input_state {
            let mut state = state
                .lock()
                .map_err(|_| "CHIP-8 live-input state is unavailable".to_string())?;
            state.key_mask = self.engine.cpu.keys.as_mask();
            state.current_frame = self.engine.frame_count();
        }
        self.scheduled_inputs = payload.scheduled_inputs;
        self.next_scheduled_input = payload.next_scheduled_input;
        self.cycles_into_frame = payload.cycles_into_frame;
        self.event_projection_fingerprint = EventProjectionFingerprintAccumulator::new();
        self.event_projection_accumulator_active = payload.event_projection_accumulator_active;
        self.next_sequence = payload.next_sequence;
        self.lifecycle = payload.lifecycle;
        Ok(())
    }
}

impl Chip8Session {
    fn step_limit_reached(&self) -> bool {
        self.max_steps
            .is_some_and(|limit| self.engine.cycle_count() >= limit)
    }

    fn steps_available_in_frame(&self) -> u32 {
        let frame_remaining = self
            .engine
            .cycles_per_frame()
            .saturating_sub(self.cycles_into_frame);
        let Some(limit) = self.max_steps else {
            return frame_remaining;
        };
        let step_remaining = limit.saturating_sub(self.engine.cycle_count());
        frame_remaining.min(u32::try_from(step_remaining).unwrap_or(u32::MAX))
    }

    fn validate_fresh_restore_target(&self) -> Result<(), String> {
        let exact_fresh_state = self.lifecycle == SessionLifecycle::Fresh
            && self.engine.cycle_count() == 0
            && self.engine.frame_count() == 0
            && snapshot_to_bytes(&self.engine.save_snapshot()) == self.initial_state_bytes
            && self.scheduled_inputs == self.initial_scheduled_inputs
            && self.next_scheduled_input == 0
            && self.cycles_into_frame == 0
            && self.event_projection_fingerprint.event_count() == 0
            && self.event_projection_accumulator_active
            && self.next_sequence == 0;
        if !exact_fresh_state {
            return Err(
                "CHIP-8 snapshot restore requires an exact fresh session; reset before restoring"
                    .into(),
            );
        }
        Ok(())
    }

    fn request_digest(&self) -> Result<ContentDigest, String> {
        canonical_json_fingerprint("glassvm.chip8.execution-request", &self.request)
    }

    fn update_event_projection_fingerprint(
        accumulator: &mut EventProjectionFingerprintAccumulator,
        event: &ExecutionEvent,
    ) -> Result<(), String> {
        let mut neutral = event.clone();
        if neutral.kind == EventKind::SnapshotCaptured
            && let Some(metadata) = neutral
                .extensions
                .get_mut("glassvm.snapshot")
                .and_then(Value::as_object_mut)
        {
            // Snapshot bytes bind the run ID so they fail closed across run
            // boundaries. Their byte digest and length are therefore envelope
            // evidence, not run-neutral trajectory semantics.
            metadata.remove("digest");
            metadata.remove("byte_len");
        }
        accumulator.update(&neutral)
    }

    fn engine_evidence(
        engine: &chip8_core::Engine,
        observation: &ObservationRequest,
    ) -> Result<ContentDigest, String> {
        let _ = observation;
        let result = Self::build_execution_result(engine, chip8_core::TerminationReason::Timeout);
        canonical_json_fingerprint("glassvm.chip8.engine-evidence", &result)
    }

    fn snapshot_payload_for(
        &self,
        lifecycle: SessionLifecycle,
    ) -> Result<Chip8SessionSnapshotPayload, String> {
        if lifecycle == SessionLifecycle::Incremental && self.engine.cpu.halted {
            return Err("CHIP-8 incremental snapshot cannot contain a halted machine".into());
        }
        Ok(Chip8SessionSnapshotPayload {
            request_digest: self.request_digest()?,
            artifact: self.artifact.clone(),
            bundle_version: VersionStamp::from(env!("CARGO_PKG_VERSION")),
            machine_version: VersionStamp::from(SEMANTICS),
            emulator_version: VersionStamp::from(EMULATOR_VERSION),
            variant: self.variant.clone(),
            effective_seed: self.effective_seed,
            initial_state_digest: ContentDigest::sha256(&self.initial_state_bytes),
            engine_state: snapshot_to_bytes(&self.engine.save_snapshot()),
            engine_cycle_count: self.engine.cycle_count(),
            engine_frame_count: self.engine.frame_count(),
            cycles_into_frame: self.cycles_into_frame,
            engine_evidence: Self::engine_evidence(&self.engine, &self.request.observation)?,
            scheduled_inputs: self.scheduled_inputs.clone(),
            next_scheduled_input: self.next_scheduled_input,
            event_projection_accumulator_active: self.event_projection_accumulator_active,
            next_sequence: self.next_sequence,
            lifecycle,
        })
    }

    fn snapshot_bytes_for(&self, lifecycle: SessionLifecycle) -> Result<Vec<u8>, String> {
        let payload = self.snapshot_payload_for(lifecycle)?;
        let payload_digest = canonical_json_fingerprint(SESSION_SNAPSHOT_PAYLOAD_DOMAIN, &payload)?;
        serde_json::to_vec(&Chip8SessionSnapshot {
            format: SESSION_SNAPSHOT_FORMAT.into(),
            version: SESSION_SNAPSHOT_FORMAT_VERSION,
            payload,
            payload_digest,
        })
        .map_err(|error| format!("CHIP-8 session snapshot serialization failed: {error}"))
    }

    fn validate_snapshot_identity(&self, snapshot: &Chip8SessionSnapshot) -> Result<(), String> {
        if snapshot.format != SESSION_SNAPSHOT_FORMAT
            || snapshot.version != SESSION_SNAPSHOT_FORMAT_VERSION
        {
            return Err(format!(
                "unsupported CHIP-8 session snapshot format {:?} version {}",
                snapshot.format, snapshot.version
            ));
        }
        let payload_digest =
            canonical_json_fingerprint(SESSION_SNAPSHOT_PAYLOAD_DOMAIN, &snapshot.payload)?;
        if snapshot.payload_digest != payload_digest {
            return Err("CHIP-8 session snapshot payload digest mismatch".into());
        }
        let payload = &snapshot.payload;
        if payload.request_digest != self.request_digest()? {
            return Err("CHIP-8 session snapshot execution-request identity mismatch".into());
        }
        if payload.artifact != self.artifact {
            return Err("CHIP-8 session snapshot ROM identity mismatch".into());
        }
        if payload.bundle_version != VersionStamp::from(env!("CARGO_PKG_VERSION"))
            || payload.machine_version != VersionStamp::from(SEMANTICS)
            || payload.emulator_version != VersionStamp::from(EMULATOR_VERSION)
            || payload.variant != self.variant
            || payload.effective_seed != self.effective_seed
            || payload.initial_state_digest != ContentDigest::sha256(&self.initial_state_bytes)
        {
            return Err("CHIP-8 session snapshot runtime identity mismatch".into());
        }
        Ok(())
    }

    fn validate_snapshot_session_state(
        &self,
        payload: &Chip8SessionSnapshotPayload,
    ) -> Result<(), String> {
        if payload.scheduled_inputs.len() < self.initial_scheduled_inputs.len()
            || payload.scheduled_inputs[..self.initial_scheduled_inputs.len()]
                != self.initial_scheduled_inputs
        {
            return Err(
                "CHIP-8 session snapshot schedule does not preserve the frozen request prefix"
                    .into(),
            );
        }
        if payload.next_scheduled_input > payload.scheduled_inputs.len() {
            return Err("CHIP-8 session snapshot schedule cursor exceeds its schedule".into());
        }
        if payload.engine_frame_count > self.max_frames {
            return Err("CHIP-8 session snapshot exceeds the execution frame budget".into());
        }
        if self
            .max_steps
            .is_some_and(|limit| payload.engine_cycle_count > limit)
        {
            return Err("CHIP-8 session snapshot exceeds the execution step budget".into());
        }
        if payload.cycles_into_frame > self.cycles_per_frame {
            return Err("CHIP-8 session snapshot has an invalid partial-frame phase".into());
        }
        let completed_cycles = payload
            .engine_frame_count
            .checked_mul(u64::from(self.cycles_per_frame))
            .ok_or_else(|| "CHIP-8 snapshot completed-cycle count overflow".to_string())?;
        let expected_cycles = completed_cycles
            .checked_add(u64::from(payload.cycles_into_frame))
            .ok_or_else(|| "CHIP-8 snapshot cycle count overflow".to_string())?;
        if payload.engine_cycle_count != expected_cycles {
            return Err(format!(
                "CHIP-8 session snapshot cycle/frame phase is inconsistent: {} cycles, {} frames, {} partial cycles",
                payload.engine_cycle_count, payload.engine_frame_count, payload.cycles_into_frame
            ));
        }
        for (index, scheduled_input) in payload.scheduled_inputs.iter().enumerate() {
            let frame = scheduled_input.frame;
            if index < payload.next_scheduled_input {
                if frame > payload.engine_frame_count {
                    return Err("CHIP-8 session snapshot consumed a future-frame stimulus".into());
                }
            } else if frame < payload.engine_frame_count
                || payload.cycles_into_frame > 0 && frame == payload.engine_frame_count
            {
                return Err(
                    "CHIP-8 session snapshot stimulus cursor is inconsistent with its frame phase"
                        .into(),
                );
            }
        }
        let exact_fresh_state = payload.engine_cycle_count == 0
            && payload.engine_frame_count == 0
            && payload.cycles_into_frame == 0
            && payload.engine_state == self.initial_state_bytes
            && payload.next_scheduled_input == 0
            && payload.scheduled_inputs == self.initial_scheduled_inputs
            && payload.event_projection_accumulator_active
            && payload.next_sequence == 0;
        match payload.lifecycle {
            SessionLifecycle::Fresh if !exact_fresh_state => {
                return Err("CHIP-8 fresh snapshot contains advanced session state".into());
            }
            SessionLifecycle::Incremental if exact_fresh_state => {
                return Err("CHIP-8 incremental snapshot contains only fresh session state".into());
            }
            SessionLifecycle::Incremental if !payload.event_projection_accumulator_active => {
                return Err(
                    "CHIP-8 incremental snapshot has a finalized event projection accumulator"
                        .into(),
                );
            }
            SessionLifecycle::Incremental if payload.engine_frame_count >= self.max_frames => {
                return Err("CHIP-8 incremental snapshot has exhausted the frame budget".into());
            }
            SessionLifecycle::Fresh
            | SessionLifecycle::Incremental
            | SessionLifecycle::Terminal => {}
        }
        Ok(())
    }

    fn reconstruct_snapshot_engine(
        &self,
        payload: &Chip8SessionSnapshotPayload,
    ) -> Result<Box<chip8_core::Engine>, String> {
        let mut engine = Box::new(chip8_core::Engine::new(
            &self.rom_bytes,
            *self.engine.quirks(),
            self.effective_seed,
        )?);
        engine.set_cycles_per_frame(self.cycles_per_frame);
        engine.set_step_recording(observation_requires_step_records(&self.request.observation));

        let mut cursor = 0_usize;
        {
            let mut apply_consumed_at_frame =
                |engine: &mut chip8_core::Engine, frame: u64| -> Result<(), String> {
                    while cursor < payload.next_scheduled_input {
                        let scheduled_input = &payload.scheduled_inputs[cursor];
                        let at = scheduled_input.frame;
                        if at > frame {
                            break;
                        }
                        if at < frame {
                            return Err(
                                "CHIP-8 snapshot replay missed a consumed stimulus boundary".into(),
                            );
                        }
                        let mut mask = engine.cpu.keys.as_mask();
                        if scheduled_input.active {
                            mask |= 1_u16 << scheduled_input.key;
                        } else {
                            mask &= !(1_u16 << scheduled_input.key);
                        }
                        engine.set_key_mask(mask);
                        cursor += 1;
                    }
                    Ok(())
                };

            for frame in 0..payload.engine_frame_count {
                apply_consumed_at_frame(&mut engine, frame)?;
                match engine.step_frame() {
                    chip8_core::FrameResult::Ok => {}
                    result => {
                        return Err(format!(
                            "CHIP-8 snapshot replay terminated before completed frame {frame}: {result:?}"
                        ));
                    }
                }
            }

            apply_consumed_at_frame(&mut engine, payload.engine_frame_count)?;
            for step in 0..payload.cycles_into_frame {
                let result = engine.step_in_frame();
                let final_partial_step = step + 1 == payload.cycles_into_frame;
                match result {
                    chip8_core::StepResult::Ok | chip8_core::StepResult::WaitingForKey => {}
                    chip8_core::StepResult::Halted if final_partial_step => {}
                    result if final_partial_step => {
                        if matches!(
                            result,
                            chip8_core::StepResult::InvalidOpcode(_)
                                | chip8_core::StepResult::StackOverflow
                                | chip8_core::StepResult::StackUnderflow
                                | chip8_core::StepResult::MemoryFault(_)
                        ) {
                            // A terminal step is valid only as the saved partial frame's final step.
                        } else {
                            return Err(format!(
                                "unsupported CHIP-8 snapshot replay step result {result:?}"
                            ));
                        }
                    }
                    result => {
                        return Err(format!(
                            "CHIP-8 snapshot replay terminated before partial step {}: {result:?}",
                            step + 1
                        ));
                    }
                }
            }
        }
        if cursor != payload.next_scheduled_input {
            return Err("CHIP-8 snapshot replay did not consume the saved schedule cursor".into());
        }
        if engine.cycle_count() != payload.engine_cycle_count
            || engine.frame_count() != payload.engine_frame_count
        {
            return Err("CHIP-8 snapshot replay produced different engine counters".into());
        }
        if snapshot_to_bytes(&engine.save_snapshot()) != payload.engine_state {
            return Err("CHIP-8 snapshot replay produced different machine state".into());
        }
        if Self::engine_evidence(&engine, &self.request.observation)? != payload.engine_evidence {
            return Err("CHIP-8 snapshot replay produced different metric evidence".into());
        }
        if payload.lifecycle == SessionLifecycle::Incremental && engine.cpu.halted {
            return Err("CHIP-8 incremental snapshot contains a halted machine".into());
        }
        Ok(engine)
    }

    fn apply_scheduled_inputs_for_current_frame(
        &mut self,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        let frame = self.engine.frame_count();
        if let Some(state) = &self.live_input_state {
            let mut state = state
                .lock()
                .map_err(|_| "CHIP-8 live-input state is unavailable".to_string())?;
            state.current_frame = frame;
            self.engine.set_key_mask(state.key_mask);
        }
        while let Some(scheduled_input) = self
            .scheduled_inputs
            .get(self.next_scheduled_input)
            .cloned()
        {
            let stimulus_frame = scheduled_input.frame;
            if stimulus_frame > frame {
                break;
            }
            if stimulus_frame < frame {
                return Err(format!(
                    "CHIP-8 stimulus {} at frame {stimulus_frame} was not applied before frame {frame}",
                    scheduled_input.ordinal
                ));
            }
            let before = self.engine.cpu.keys.as_mask();
            let mask = if scheduled_input.active {
                before | (1_u16 << scheduled_input.key)
            } else {
                before & !(1_u16 << scheduled_input.key)
            };
            self.engine.set_key_mask(mask);

            let input_id = InputId::new(format!("chip8.key.{:x}", scheduled_input.key))
                .map_err(|error| format!("invalid prepared CHIP-8 input ID: {error}"))?;
            let input_schema = schema("chip8.input.key");
            let source = InputSource::Scheduled {
                ordinal: scheduled_input.ordinal,
            };
            let application_coordinate = InputCoordinate::frame(frame);
            let applied = InputApplied {
                input_id: input_id.clone(),
                input_schema: input_schema.clone(),
                source,
                application_coordinate: application_coordinate.clone(),
            };

            let context = EventContext {
                arch: MachineId::from(CHIP8_ID),
                machine_version: VersionStamp::from(SEMANTICS),
                run_id: self.request.run_id.clone(),
                sequence: self.next_sequence,
                step: self.engine.cycle_count(),
                cycle_or_tick: Some(self.engine.cycle_count()),
                frame: Some(frame),
                pc: None,
                instruction: None,
            };
            let mut event = ExecutionEvent::from_context(&context, EventKind::InputApplied);
            event.extensions.insert(
                "glassvm.input_applied".into(),
                serde_json::to_value(&applied)
                    .map_err(|error| format!("encode CHIP-8 InputApplied: {error}"))?,
            );
            event.reads.push(observed_read(
                key_mask_location(),
                json!(before),
                self.request.observation.normalized_events.access_detail,
            ));
            event.writes.push(observed_write(
                key_mask_location(),
                json!(before),
                json!(mask),
                self.request.observation.normalized_events.access_detail,
            ));
            event.io.push(IoObservation {
                port: "keypad_in".into(),
                direction: IoDirection::Input,
                channel: IoChannel::Keypad,
                value: json!({"input_id": input_id, "ordinal": scheduled_input.ordinal}),
            });
            self.emit_selected(event, sink)?;
            if self
                .prepared_observation
                .input_value_ids
                .iter()
                .any(|selected| selected == &input_id)
            {
                let payload = TypedInputPayload::new(
                    input_schema.clone(),
                    StructuredValue::Bool(scheduled_input.active),
                )?;
                let evidence = InputValueEvidence::new(
                    input_id,
                    input_schema,
                    source,
                    application_coordinate,
                    payload,
                );
                sink.emit(Emission::InputValueEvidence(&evidence))
                    .map_err(sink_error)?;
            }
            self.next_scheduled_input += 1;
        }
        Ok(())
    }

    fn emit_pending_step_records(&mut self, sink: &mut dyn EmissionSink) -> Result<(), String> {
        for record in self.engine.drain_step_records() {
            let context = self.step_context(&record);
            let step_sequence = self.emit_selected(
                ExecutionEvent::from_context(&context, EventKind::StepStarted),
                sink,
            )?;

            let instruction_sequence = if record.opcode.is_some() {
                let mut decoded =
                    ExecutionEvent::from_context(&context, EventKind::InstructionDecoded);
                decoded.reads = chip8_instruction_reads(
                    &record,
                    self.engine.quirks(),
                    self.request.observation.normalized_events.access_detail,
                );
                self.emit_selected(decoded, sink)?
            } else {
                None
            };
            let source_sequence = instruction_sequence.or(step_sequence);
            let mut input_sample_sequence = None;

            if let Some(mut event) = waiting_input_sample_event(
                &context,
                &record,
                self.request.observation.normalized_events.access_detail,
            ) {
                if let Some(source_sequence) = source_sequence {
                    event.causes.push(CausalLink {
                        source_sequence,
                        relation: CausalRelation::Input,
                        evidence: Some("opcode-less CHIP-8 key-wait sampling step".into()),
                    });
                }
                input_sample_sequence = self.emit_selected(event, sink)?;
            }

            for native_event in &record.events {
                let native = native_event_from_step(native_event, &record);
                let mut normalized = Chip8NativeEventAdapter.normalize(&context, &native)?;
                for event in &mut normalized {
                    enrich_normalized_event(
                        event,
                        native_event,
                        &record,
                        self.request.observation.normalized_events.access_detail,
                    );
                    if let Some(source_sequence) = source_sequence {
                        event.causes.push(CausalLink {
                            source_sequence,
                            relation: causal_relation_for(&event.kind),
                            evidence: Some("same CHIP-8 execution step".into()),
                        });
                    }
                    if !self
                        .request
                        .observation
                        .native_evidence
                        .includes(&native.kind)
                        || !self.request.observation.native_evidence.enabled
                    {
                        event.extensions.clear();
                    }
                }
                for event in normalized {
                    let is_input_sample = event.kind == EventKind::InputSampled;
                    let sequence = self.emit_selected(event, sink)?;
                    if is_input_sample && sequence.is_some() {
                        input_sample_sequence = sequence;
                    }
                }
            }

            for mut event in [
                conditional_branch_event(&context, &record),
                randomness_sample_event(&context, &record),
                fatal_step_trap(&context, &record),
            ]
            .into_iter()
            .flatten()
            {
                if let Some(source_sequence) = source_sequence {
                    event.causes.push(CausalLink {
                        source_sequence,
                        relation: causal_relation_for(&event.kind),
                        evidence: Some("same CHIP-8 execution step".into()),
                    });
                }
                self.emit_selected(event, sink)?;
            }

            if self.request.observation.normalized_events.state_diffs {
                for mut event in state_delta_events(
                    &context,
                    &record,
                    self.request.observation.normalized_events.access_detail,
                ) {
                    if let Some(source_sequence) = source_sequence {
                        event.causes.push(CausalLink {
                            source_sequence,
                            relation: CausalRelation::Data,
                            evidence: Some(
                                "state delta from the same CHIP-8 execution step".into(),
                            ),
                        });
                    }
                    if !event.writes.is_empty()
                        && record.opcode.is_none()
                        && !matches!(record.before.key_wait, chip8_core::KeyWait::None)
                        && let Some(source_sequence) = input_sample_sequence
                    {
                        event.causes.push(CausalLink {
                            source_sequence,
                            relation: CausalRelation::Input,
                            evidence: Some(
                                "keypad sample caused this wait-state transition".into(),
                            ),
                        });
                    }
                    self.emit_selected(event, sink)?;
                }
            }
        }
        Ok(())
    }

    fn emit_pending_frame_timer_records(
        &mut self,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        for record in self.engine.drain_frame_timer_records() {
            let context = EventContext {
                arch: MachineId::from(CHIP8_ID),
                machine_version: VersionStamp::from(SEMANTICS),
                run_id: self.request.run_id.clone(),
                sequence: self.next_sequence,
                step: record.step,
                cycle_or_tick: Some(record.step),
                frame: Some(record.frame),
                pc: None,
                instruction: None,
            };
            if record.before.sound > 0 {
                let mut sound = ExecutionEvent::from_context(&context, EventKind::SoundEmitted);
                sound.reads.push(observed_read(
                    timer_location("sound"),
                    json!(record.before.sound),
                    self.request.observation.normalized_events.access_detail,
                ));
                sound.reads.push(observed_read(
                    named_location(StateSpace::Extension("chip8.audio".into()), "pattern", 128),
                    json!(self.engine.cpu.audio_buf),
                    self.request.observation.normalized_events.access_detail,
                ));
                sound.reads.push(observed_read(
                    named_location(StateSpace::Output, "audio_pitch", 8),
                    json!(self.engine.cpu.audio_pitch),
                    self.request.observation.normalized_events.access_detail,
                ));
                sound.io.push(IoObservation {
                    port: "sound_out".into(),
                    direction: IoDirection::Output,
                    channel: IoChannel::Sound,
                    value: json!({
                        "active": true,
                        "timer_before": record.before.sound,
                        "timer_after": record.after.sound,
                    }),
                });
                self.emit_selected(sound, sink)?;
            }

            let mut event = ExecutionEvent::from_context(&context, EventKind::TimerChanged);
            if record.before.delay != record.after.delay {
                event.reads.push(observed_read(
                    timer_location("delay"),
                    json!(record.before.delay),
                    self.request.observation.normalized_events.access_detail,
                ));
                event.writes.push(observed_write(
                    timer_location("delay"),
                    json!(record.before.delay),
                    json!(record.after.delay),
                    self.request.observation.normalized_events.access_detail,
                ));
            }
            if record.before.sound != record.after.sound {
                event.reads.push(observed_read(
                    timer_location("sound"),
                    json!(record.before.sound),
                    self.request.observation.normalized_events.access_detail,
                ));
                event.writes.push(observed_write(
                    timer_location("sound"),
                    json!(record.before.sound),
                    json!(record.after.sound),
                    self.request.observation.normalized_events.access_detail,
                ));
            }
            event.io.push(IoObservation {
                port: "timer_out".into(),
                direction: IoDirection::Output,
                channel: IoChannel::Timer,
                value: json!({
                    "delay_before": record.before.delay,
                    "delay_after": record.after.delay,
                    "sound_before": record.before.sound,
                    "sound_after": record.after.sound,
                    "source": "frame_tick",
                }),
            });
            self.emit_selected(event, sink)?;
        }
        Ok(())
    }

    fn emit_selected(
        &mut self,
        mut event: ExecutionEvent,
        sink: &mut dyn EmissionSink,
    ) -> Result<Option<u64>, String> {
        if !self
            .request
            .observation
            .normalized_events
            .events
            .includes(&event.kind)
        {
            return Ok(None);
        }
        event.sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| "CHIP-8 event sequence overflow".to_string())?;
        let sequence = event.sequence;
        Self::update_event_projection_fingerprint(&mut self.event_projection_fingerprint, &event)?;
        sink.emit(Emission::Event(&event)).map_err(sink_error)?;
        Ok(Some(sequence))
    }

    fn step_context(&self, record: &chip8_core::StepTraceRecord) -> EventContext {
        EventContext {
            arch: MachineId::from(CHIP8_ID),
            machine_version: VersionStamp::from(SEMANTICS),
            run_id: self.request.run_id.clone(),
            sequence: self.next_sequence,
            step: record.step,
            cycle_or_tick: Some(record.step),
            frame: record.frame,
            pc: Some(Address::new("program", record.pc as u64)),
            instruction: chip8_instruction_ref(record),
        }
    }

    fn emit_frame_completed(&mut self, sink: &mut dyn EmissionSink) -> Result<(), String> {
        let context = EventContext {
            frame: self.engine.frame_count().checked_sub(1),
            pc: None,
            ..self.event_context()
        };
        let mut event = ExecutionEvent::from_context(&context, EventKind::FrameCompleted);
        let frame_sequence = self.next_sequence;
        event.sequence = frame_sequence;
        // Completion is a temporal fact. Frame evidence is emitted separately
        // and remains available even when normalized events were not selected.
        self.emit_selected(event, sink)?;
        let frame =
            self.engine.frame_count().checked_sub(1).ok_or_else(|| {
                "CHIP-8 emitted frame evidence before completing a frame".to_string()
            })?;
        let artifact = match self.request.observation.frames.capture {
            FrameCapture::None => return Ok(()),
            FrameCapture::Hashes => FrameArtifact::fingerprint(
                self.request.run_id.clone(),
                MachineId::from(CHIP8_ID),
                VersionStamp::from(SEMANTICS),
                schema("chip8.frame_fingerprint"),
                frame_sequence,
                self.engine.cycle_count(),
                frame,
                self.engine.frame_hash().to_le_bytes().to_vec(),
            ),
            FrameCapture::Full => FrameArtifact::full(
                self.request.run_id.clone(),
                MachineId::from(CHIP8_ID),
                VersionStamp::from(SEMANTICS),
                schema("chip8.physical_backing_pixels.plane_major"),
                frame_sequence,
                self.engine.cycle_count(),
                frame,
                self.engine.framebuffer_flat(),
            ),
        };
        sink.emit(Emission::Frame(&artifact)).map_err(sink_error)
    }

    fn emit_lifecycle_event(
        &mut self,
        kind: EventKind,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        let context = self.event_context();
        let event = ExecutionEvent::from_context(&context, kind);
        self.emit_selected(event, sink).map(|_| ())
    }

    fn emit_periodic_snapshot(
        &mut self,
        lifecycle: SessionLifecycle,
        sink: &mut dyn EmissionSink,
    ) -> Result<Option<SnapshotArtifact>, String> {
        let SnapshotCapture::EverySteps(interval) = self.request.observation.snapshots.capture
        else {
            return Ok(None);
        };
        let cycles = self.engine.cycle_count();
        if cycles == 0 || !cycles.is_multiple_of(interval) {
            return Ok(None);
        }
        self.emit_snapshot(lifecycle, sink).map(Some)
    }

    fn emit_snapshot(
        &mut self,
        lifecycle: SessionLifecycle,
        sink: &mut dyn EmissionSink,
    ) -> Result<SnapshotArtifact, String> {
        let snapshot = SnapshotArtifact::new_typed(
            self.request.run_id.clone(),
            MachineId::from(CHIP8_ID),
            VersionStamp::from(SEMANTICS),
            session_snapshot_schema(),
            self.next_sequence,
            self.engine.cycle_count(),
            self.snapshot_bytes_for(lifecycle)?,
        );
        let context = EventContext {
            pc: None,
            instruction: None,
            ..self.event_context()
        };
        let mut event = ExecutionEvent::from_context(&context, EventKind::SnapshotCaptured);
        event.extensions.insert(
            "glassvm.snapshot".into(),
            json!({
                "state_schema": snapshot.state_schema,
                "byte_len": snapshot.byte_len,
                "digest": snapshot.digest,
            }),
        );
        self.emit_selected(event, sink)?;
        sink.emit(Emission::Snapshot(&snapshot))
            .map_err(sink_error)?;
        Ok(snapshot)
    }

    fn event_context(&self) -> EventContext {
        EventContext {
            arch: MachineId::from(CHIP8_ID),
            machine_version: VersionStamp::from(SEMANTICS),
            run_id: self.request.run_id.clone(),
            sequence: self.next_sequence,
            step: self.engine.cycle_count(),
            cycle_or_tick: Some(self.engine.cycle_count()),
            frame: Some(self.engine.frame_count()),
            pc: Some(Address::new("program", self.engine.cpu.pc as u64)),
            instruction: None,
        }
    }

    #[doc(hidden)]
    pub fn build_result(
        &self,
        termination: chip8_core::TerminationReason,
    ) -> Result<RunResult, String> {
        Ok(Self::build_execution_result(&self.engine, termination))
    }

    fn build_execution_result(
        engine: &chip8_core::Engine,
        termination: chip8_core::TerminationReason,
    ) -> RunResult {
        let summary = engine.build_summary(termination);
        RunResult {
            common: CommonMetrics {
                cycles: summary.cycles,
                frames: summary.frames,
                termination: termination_to_string(&summary.termination),
                boot_success: summary.boot_success,
            },
            capabilities: Vec::new(),
        }
    }

    fn native_evidence_for_engine(
        engine: &chip8_core::Engine,
        run_id: &glassvm_core::RunId,
        termination: chip8_core::TerminationReason,
    ) -> Result<Vec<NativeEvidenceEnvelope>, String> {
        let summary = engine.build_summary(termination);
        let coverage = &summary.coverage;
        let interestingness = engine.compute_interestingness();
        let trajectory_identity = engine.trajectory_identity();
        let envelope =
            |sequence: u64, kind: &str, schema: glassvm_core::SchemaRef, payload: Value| {
                NativeEvidenceEnvelope::new(
                    run_id.clone(),
                    MachineId::from(CHIP8_ID),
                    VersionStamp::from(SEMANTICS),
                    sequence,
                    engine.cycle_count(),
                    glassvm_core::NativeEvent {
                        schema,
                        kind: kind.into(),
                        payload,
                    },
                )
            };
        let observations = vec![
            envelope(
                0,
                "chip8.framebuffer",
                schema("chip8.framebuffer"),
                json!({
                    "width": 128,
                    "height": 64,
                    "planes": 2,
                    "bytes": engine.framebuffer_flat(),
                }),
            ),
            envelope(
                1,
                "chip8.execution_summary",
                schema("chip8.execution_summary"),
                json!({
                    "rom_hash": summary.rom_hash,
                    "draw_count": summary.draw_count,
                    "collision_count": summary.collision_count,
                    "input_opcode_count": summary.input_opcode_count,
                    "unique_frame_count": summary.unique_frame_count,
                }),
            ),
            envelope(
                2,
                "chip8.interestingness",
                interestingness_schema(),
                json!({
                    "frame_entropy": interestingness.frame_entropy,
                    "change_rate": interestingness.change_rate,
                    "last_change_frame": interestingness.last_change_frame,
                    "late_change_rate": interestingness.late_change_rate,
                    "late_frame_discovery_rate": interestingness.late_frame_discovery_rate,
                    "opcode_diversity": interestingness.opcode_diversity,
                    "coverage_growth": interestingness.coverage_growth,
                    "input_responsiveness": interestingness.input_responsiveness,
                    "draw_density": interestingness.draw_density,
                    "mean_lit_fraction": interestingness.mean_lit_fraction,
                    "peak_lit_fraction": interestingness.peak_lit_fraction,
                    "mean_frame_delta": interestingness.mean_frame_delta,
                    "mean_capped_changed_pixels": interestingness.mean_capped_changed_pixels,
                    "broad_transition_share": interestingness.broad_transition_share,
                    "motion_spread": interestingness.motion_spread,
                    "motion_spatial_spread": interestingness.motion_spatial_spread,
                    "motion_axis_balance": interestingness.motion_axis_balance,
                    "frame_delta_cv": interestingness.frame_delta_cv,
                    "mean_edge_density": interestingness.mean_edge_density,
                    "ordered_spatial_structure": interestingness.ordered_spatial_structure,
                    "object_scale_composition": interestingness.object_scale_composition,
                    "repetitive_texture": interestingness.repetitive_texture,
                    "spatial_repeat_autocorrelation": interestingness.spatial_repeat_autocorrelation,
                    "persistent_component_structure": interestingness.persistent_component_structure,
                    "connected_negative_space": interestingness.connected_negative_space,
                    "active_region_fraction": interestingness.active_region_fraction,
                    "mean_change_region_fraction": interestingness.mean_change_region_fraction,
                    "loop_period_frames": interestingness.loop_period_frames,
                    "loop_periodicity": interestingness.loop_periodicity,
                    "clear_count": interestingness.clear_count,
                    "delay_timer_set_count": interestingness.delay_timer_set_count,
                    "delay_timer_nonzero_count": interestingness.delay_timer_nonzero_count,
                    "sound_timer_set_count": interestingness.sound_timer_set_count,
                    "sound_timer_nonzero_count": interestingness.sound_timer_nonzero_count,
                    "scroll_count": interestingness.scroll_count,
                    "composition_stable_foreground": interestingness.composition_stable_foreground,
                    "composition_active_fraction": interestingness.composition_active_fraction,
                    "coherent_change_topology": interestingness.coherent_change_topology,
                    "temporal_overlap_reversal": interestingness.temporal_overlap_reversal,
                }),
            ),
            envelope(
                3,
                "chip8.coverage",
                schema("chip8.coverage"),
                json!({
                    "unique_pcs": coverage.unique_pcs,
                    "unique_edges": coverage.unique_edges,
                    "memory_written_bytes": coverage.memory_written_bytes,
                    "max_stack_depth": coverage.max_stack_depth,
                }),
            ),
            envelope(
                4,
                "chip8.trajectory_identity",
                schema("chip8.trajectory_identity"),
                json!({
                    "definition": trajectory_identity.definition,
                    "digest": trajectory_identity.digest,
                    "frame_count": trajectory_identity.frame_count,
                }),
            ),
        ];
        Ok(observations)
    }
}
