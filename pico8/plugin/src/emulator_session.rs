use std::collections::BTreeMap;

use glassvm_core::{
    ArtifactIdentity, CommonMetrics, ContentDigest, Emission, EmissionSink, EmulatorSession,
    EventContext, EventKind, ExecutionBudget, ExecutionEvent, ExecutionRequest, FrameCapture,
    InitialStateIdentity, InitialStateKind, MachineId, MachineRuntimeIdentity, NativeEventAdapter,
    NativeObservation, ObservableAdapter, ReplayFingerprints, ReplayManifest, ReplayPackage,
    ReplayReceipt, ReplayStimulus, RunResult, SeedIdentity, SnapshotArtifact, SnapshotCapture,
    StimulusIdentity, TemporalCoordinate, TimingIdentity, TraceChunk, TraceFingerprintAccumulator,
    VersionStamp, canonical_json_bytes, report_fingerprint, state_fingerprint,
};
use pico8_core::{Cartridge, DISPLAY_HEIGHT, DISPLAY_WIDTH, Pico8Runtime, RAM_BYTES};
use serde_json::{Value, json};

use crate::adapters::{Pico8NativeEventAdapter, Pico8ObservableAdapter};
use crate::identity::{
    CONTINUATION_SCHEMA_VERSION, EMULATOR_VERSION, MACHINE_VERSION, MAX_CONTINUATION_BYTES,
    PICO8_ID, component_identity, format_name, schema,
};
use crate::session_snapshot::{
    Pico8Continuation, SessionLifecycle, continuation_integrity, observable_state_bytes,
    request_digest,
};
use crate::trace::sink_error;

pub(super) struct Pico8Session {
    pub(super) request: ExecutionRequest,
    pub(super) cartridge: Cartridge,
    pub(super) artifact_bytes: Vec<u8>,
    pub(super) runtime: Pico8Runtime,
    pub(super) instruction_budget: u64,
    pub(super) initial_state_bytes: Vec<u8>,
    pub(super) stimuli: Vec<ReplayStimulus>,
    pub(super) next_stimulus: usize,
    pub(super) next_sequence: u64,
    pub(super) trace_fingerprint: TraceFingerprintAccumulator,
    pub(super) pending_trace_events: Vec<ExecutionEvent>,
    pub(super) next_trace_chunk: u64,
    pub(super) previous_trace_chunk_digest: Option<ContentDigest>,
    pub(super) frame_hashes: Vec<String>,
    pub(super) lifecycle: SessionLifecycle,
    pub(super) runtime_error: Option<String>,
}

impl EmulatorSession for Pico8Session {
    fn request(&self) -> &ExecutionRequest {
        &self.request
    }

    fn execute(&mut self, sink: &mut dyn EmissionSink) -> Result<RunResult, String> {
        if self.lifecycle != SessionLifecycle::Fresh || self.runtime.frame() != 0 {
            return Err(
                "replayable PICO-8 execution must begin at boot; reset after stepping or changing input"
                    .into(),
            );
        }
        // The outer lifecycle guard makes every failed execute attempt terminal,
        // while snapshots emitted before the frame budget are resumable.
        self.lifecycle = SessionLifecycle::Incremental;
        let outcome = (|| -> Result<RunResult, String> {
            self.runtime_error = None;
            self.reset_emission_state();
            let manifest = self.replay_manifest()?;
            sink.emit(Emission::RunStarted(&self.request))
                .map_err(sink_error)?;
            self.emit_event(EventKind::RunStarted, None, None, sink)?;

            for _ in 0..self.request.config.max_frames {
                self.apply_stimuli(sink)?;
                if let Err(error) = self.runtime.step_frame() {
                    self.runtime_error = Some(error);
                    break;
                }
                self.emit_frame(sink)?;
                if let SnapshotCapture::EverySteps(interval) = self.request.observation.snapshots
                    && self.runtime.frame().is_multiple_of(interval)
                {
                    if self.runtime.frame() >= self.request.config.max_frames {
                        self.lifecycle = SessionLifecycle::Terminal;
                    }
                    self.emit_snapshot(sink)?;
                }
            }

            if matches!(self.request.observation.snapshots, SnapshotCapture::Final) {
                self.lifecycle = SessionLifecycle::Terminal;
                self.emit_snapshot(sink)?;
            }
            let runtime_error = self.runtime_error.clone();
            let termination = if runtime_error.is_some() {
                "runtime_error"
            } else {
                "frame_budget"
            };
            let terminal_kind = if runtime_error.is_some() {
                EventKind::RunCrashed
            } else {
                EventKind::RunHalted
            };
            let terminal_extension = runtime_error
                .as_ref()
                .map(|error| json!({"error": error, "fatal": true}));
            self.emit_event(terminal_kind, None, terminal_extension, sink)?;
            self.flush_trace_chunk(sink)?;

            let result = self.build_result(termination, runtime_error.as_deref())?;
            if self.request.observation.collect_summaries {
                for capability in &result.capabilities {
                    sink.emit(Emission::Capability(capability))
                        .map_err(sink_error)?;
                }
            }
            let trace_event_count = self.trace_fingerprint.event_count();
            let trace = std::mem::take(&mut self.trace_fingerprint).finish();
            let final_state_bytes = observable_state_bytes(&self.runtime.snapshot())?;
            let receipt = ReplayReceipt::new(
                self.request.run_id.clone(),
                manifest.fingerprint()?,
                self.runtime.frame(),
                None,
                Some(self.runtime.frame()),
                Some(self.runtime.frame()),
                termination,
                trace_event_count,
                ReplayFingerprints {
                    final_state: state_fingerprint(
                        &schema("pico8.state.observable"),
                        &final_state_bytes,
                    ),
                    trace,
                    report: report_fingerprint(&result)?,
                },
            );
            let package = ReplayPackage::new(
                self.request.run_id.clone(),
                manifest.clone(),
                self.stimuli.clone(),
                None,
                receipt.clone(),
            );
            package
                .validate_internal()
                .into_result()
                .map_err(|comparison| {
                    format!(
                        "generated PICO-8 replay package is internally invalid: {:?}",
                        comparison.mismatches
                    )
                })?;
            sink.emit(Emission::ReplayManifest(&manifest))
                .map_err(sink_error)?;
            sink.emit(Emission::ReplayReceipt(&receipt))
                .map_err(sink_error)?;
            sink.emit(Emission::ReplayPackage(&package))
                .map_err(sink_error)?;
            sink.emit(Emission::RunFinished(&result))
                .map_err(sink_error)?;
            Ok(result)
        })();
        self.lifecycle = SessionLifecycle::Terminal;
        outcome
    }

    fn step_frame(&mut self) -> Result<(), String> {
        if let Some(error) = &self.runtime_error {
            return Err(format!(
                "PICO-8 session is terminal after runtime failure; reset before stepping again: {error}"
            ));
        }
        if self.runtime.frame() >= self.request.config.max_frames {
            return Err(format!(
                "PICO-8 frame budget {} is exhausted",
                self.request.config.max_frames
            ));
        }
        if self.lifecycle == SessionLifecycle::Terminal {
            return Err("PICO-8 session is terminal; reset before stepping again".into());
        }
        self.lifecycle = SessionLifecycle::Incremental;
        self.apply_stimuli_silently()?;
        match self.runtime.step_frame() {
            Ok(()) => {
                if self.runtime.frame() >= self.request.config.max_frames {
                    self.lifecycle = SessionLifecycle::Terminal;
                }
                Ok(())
            }
            Err(error) => {
                self.runtime_error = Some(error.clone());
                self.lifecycle = SessionLifecycle::Terminal;
                Err(error)
            }
        }
    }

    fn reset(&mut self) -> Result<(), String> {
        self.runtime = Pico8Runtime::new(
            &self.cartridge,
            self.request.config.seed,
            self.instruction_budget,
        )?;
        self.initial_state_bytes = observable_state_bytes(&self.runtime.snapshot())?;
        self.stimuli = self.request.stimuli.clone();
        self.reset_emission_state();
        self.lifecycle = SessionLifecycle::Fresh;
        self.runtime_error = None;
        Ok(())
    }

    fn snapshot(&self) -> Result<Vec<u8>, String> {
        self.continuation_bytes()
    }

    fn restore_snapshot(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.lifecycle != SessionLifecycle::Fresh {
            return Err(
                "PICO-8 session must be reset to fresh state before restoring a snapshot".into(),
            );
        }
        let continuation = self.decode_continuation(bytes)?;
        let applied_stimuli = usize::try_from(continuation.applied_stimuli).map_err(|_| {
            "PICO-8 continuation stimulus cursor does not fit this host".to_string()
        })?;
        let runtime = self.reconstruct_runtime(&continuation, applied_stimuli)?;

        self.runtime = runtime;
        self.stimuli = continuation.stimuli;
        self.reset_emission_state();
        self.next_stimulus = applied_stimuli;
        self.lifecycle = continuation.lifecycle;
        self.runtime_error = continuation.runtime_error;
        Ok(())
    }

    fn set_input_mask(&mut self, mask: u64) -> Result<(), String> {
        if self.lifecycle == SessionLifecycle::Terminal {
            return Err("PICO-8 session is terminal; reset before changing input".into());
        }
        if let Some(error) = &self.runtime_error {
            return Err(format!(
                "PICO-8 session is terminal after runtime failure; reset before changing input: {error}"
            ));
        }
        if self.runtime.frame() >= self.request.config.max_frames {
            return Err(format!(
                "PICO-8 frame budget {} is exhausted",
                self.request.config.max_frames
            ));
        }
        if self.next_stimulus != self.stimuli.len() {
            return Err(
                "PICO-8 cannot inject an immediate input while scheduled stimuli remain pending"
                    .into(),
            );
        }
        let mask = u16::try_from(mask)
            .map_err(|_| format!("PICO-8 controller mask {mask:#x} exceeds 16 bits"))?;
        self.runtime.set_input_mask(mask);
        self.lifecycle = SessionLifecycle::Incremental;
        let ordinal = u64::try_from(self.stimuli.len())
            .map_err(|_| "PICO-8 stimulus count does not fit u64".to_string())?;
        self.stimuli.push(ReplayStimulus {
            ordinal,
            coordinate: TemporalCoordinate::frame_start(self.runtime.frame()),
            port: "controllers_in".into(),
            value: json!({"mask": mask}),
        });
        self.next_stimulus = self.stimuli.len();
        Ok(())
    }
}

impl Pico8Session {
    fn continuation_bytes(&self) -> Result<Vec<u8>, String> {
        let applied_stimuli = u64::try_from(self.next_stimulus)
            .map_err(|_| "PICO-8 stimulus cursor does not fit u64".to_string())?;
        let mut continuation = Pico8Continuation {
            schema_version: CONTINUATION_SCHEMA_VERSION,
            machine_version: MACHINE_VERSION.into(),
            emulator_version: EMULATOR_VERSION.into(),
            artifact_digest: ContentDigest::sha256(&self.artifact_bytes),
            request_digest: request_digest(&self.request)?,
            runtime: self.runtime.snapshot(),
            stimuli: self.stimuli.clone(),
            applied_stimuli,
            lifecycle: self.lifecycle,
            runtime_error: self.runtime_error.clone(),
            integrity_digest: ContentDigest::sha256(&[]),
        };
        continuation.integrity_digest = continuation_integrity(&continuation)?;
        canonical_json_bytes(&continuation)
    }

    fn decode_continuation(&self, bytes: &[u8]) -> Result<Pico8Continuation, String> {
        if bytes.len() > MAX_CONTINUATION_BYTES {
            return Err(format!(
                "PICO-8 continuation is {} bytes; maximum is {MAX_CONTINUATION_BYTES}",
                bytes.len()
            ));
        }
        let continuation: Pico8Continuation = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid PICO-8 continuation: {error}"))?;
        if canonical_json_bytes(&continuation)?.as_slice() != bytes {
            return Err("PICO-8 continuation is not canonical JSON".into());
        }
        if continuation.schema_version != CONTINUATION_SCHEMA_VERSION {
            return Err(format!(
                "unsupported PICO-8 continuation schema version {}",
                continuation.schema_version
            ));
        }
        if continuation.integrity_digest != continuation_integrity(&continuation)? {
            return Err("PICO-8 continuation integrity digest mismatch".into());
        }
        if continuation.machine_version != MACHINE_VERSION {
            return Err(format!(
                "PICO-8 continuation machine version {:?} does not match {:?}",
                continuation.machine_version, MACHINE_VERSION
            ));
        }
        if continuation.emulator_version != EMULATOR_VERSION {
            return Err(format!(
                "PICO-8 continuation emulator version {:?} does not match {:?}",
                continuation.emulator_version, EMULATOR_VERSION
            ));
        }
        if continuation.artifact_digest != ContentDigest::sha256(&self.artifact_bytes) {
            return Err("PICO-8 continuation cartridge identity mismatch".into());
        }
        if continuation.request_digest != request_digest(&self.request)? {
            return Err("PICO-8 continuation execution-request identity mismatch".into());
        }
        if continuation.runtime.schema_version != 1 {
            return Err(format!(
                "unsupported PICO-8 runtime snapshot version {}",
                continuation.runtime.schema_version
            ));
        }
        if continuation.runtime.ram.len() != RAM_BYTES {
            return Err(format!(
                "PICO-8 continuation RAM has {} bytes; expected {RAM_BYTES}",
                continuation.runtime.ram.len()
            ));
        }
        if continuation.runtime.frame > self.request.config.max_frames {
            return Err(format!(
                "PICO-8 continuation frame {} exceeds request budget {}",
                continuation.runtime.frame, self.request.config.max_frames
            ));
        }
        validate_stimuli(&continuation.stimuli, self.request.config.max_frames)?;
        if continuation.stimuli.len() < self.request.stimuli.len()
            || continuation.stimuli[..self.request.stimuli.len()] != self.request.stimuli
        {
            return Err(
                "PICO-8 continuation stimulus history does not preserve the frozen request".into(),
            );
        }
        let applied_stimuli = usize::try_from(continuation.applied_stimuli).map_err(|_| {
            "PICO-8 continuation stimulus cursor does not fit this host".to_string()
        })?;
        if applied_stimuli > continuation.stimuli.len() {
            return Err(format!(
                "PICO-8 continuation stimulus cursor {applied_stimuli} exceeds history length {}",
                continuation.stimuli.len()
            ));
        }
        for (index, stimulus) in continuation.stimuli.iter().enumerate() {
            let (frame, _) = decode_stimulus(stimulus)?;
            if index < applied_stimuli && frame > continuation.runtime.frame {
                return Err(format!(
                    "PICO-8 continuation marks future stimulus {index} at frame {frame} as applied"
                ));
            }
            if index >= applied_stimuli && frame < continuation.runtime.frame {
                return Err(format!(
                    "PICO-8 continuation leaves past stimulus {index} at frame {frame} unapplied"
                ));
            }
        }
        if continuation.lifecycle == SessionLifecycle::Fresh
            && (continuation.runtime.frame != 0
                || applied_stimuli != 0
                || continuation.runtime_error.is_some()
                || continuation.stimuli != self.request.stimuli)
        {
            return Err(
                "PICO-8 continuation claims a clean boot with advanced session state".into(),
            );
        }
        if continuation.lifecycle == SessionLifecycle::Incremental
            && (continuation.runtime.frame >= self.request.config.max_frames
                || continuation.runtime_error.is_some())
        {
            return Err(
                "PICO-8 continuation claims an incremental lifecycle at a terminal state".into(),
            );
        }
        if continuation
            .runtime_error
            .as_ref()
            .is_some_and(String::is_empty)
        {
            return Err("PICO-8 continuation contains an empty runtime error".into());
        }
        if continuation.runtime_error.is_some()
            && (continuation.lifecycle != SessionLifecycle::Terminal
                || continuation.runtime.frame >= self.request.config.max_frames)
        {
            return Err("PICO-8 continuation has an impossible runtime-failure phase".into());
        }
        Ok(continuation)
    }

    fn reconstruct_runtime(
        &self,
        continuation: &Pico8Continuation,
        applied_stimuli: usize,
    ) -> Result<Pico8Runtime, String> {
        let runtime = Pico8Runtime::new(
            &self.cartridge,
            self.request.config.seed,
            self.instruction_budget,
        )?;
        let mut replay_cursor = 0;
        while runtime.frame() < continuation.runtime.frame {
            apply_replayed_stimuli(
                &runtime,
                &continuation.stimuli,
                applied_stimuli,
                &mut replay_cursor,
            )?;
            runtime.step_frame().map_err(|error| {
                format!(
                    "PICO-8 continuation replay failed before frame {}: {error}",
                    continuation.runtime.frame
                )
            })?;
        }
        apply_replayed_stimuli(
            &runtime,
            &continuation.stimuli,
            applied_stimuli,
            &mut replay_cursor,
        )?;
        if let Some(expected_error) = &continuation.runtime_error {
            let actual_error = match runtime.step_frame() {
                Ok(()) => {
                    return Err(
                        "PICO-8 continuation claims a runtime error but deterministic replay succeeded"
                            .into(),
                    );
                }
                Err(error) => error,
            };
            if &actual_error != expected_error {
                return Err(format!(
                    "PICO-8 continuation runtime error mismatch: expected {expected_error:?}, replay produced {actual_error:?}"
                ));
            }
        }
        if replay_cursor != applied_stimuli {
            return Err(format!(
                "PICO-8 continuation replay consumed {replay_cursor} stimuli; expected {applied_stimuli}"
            ));
        }
        if runtime.snapshot() != continuation.runtime {
            return Err(
                "PICO-8 continuation state does not match deterministic cartridge replay".into(),
            );
        }
        Ok(runtime)
    }

    fn reset_emission_state(&mut self) {
        self.next_stimulus = 0;
        self.next_sequence = 0;
        self.trace_fingerprint = TraceFingerprintAccumulator::new();
        self.pending_trace_events.clear();
        self.next_trace_chunk = 0;
        self.previous_trace_chunk_digest = None;
        self.frame_hashes.clear();
    }

    pub(crate) fn replay_manifest(&self) -> Result<ReplayManifest, String> {
        validate_stimuli(&self.stimuli, self.request.config.max_frames)?;
        let fps = self.runtime.fps();
        let episode = self
            .request
            .episode
            .as_ref()
            .ok_or_else(|| "PICO-8 execution lacks a resolved episode context".to_string())?;
        Ok(ReplayManifest::new(
            ArtifactIdentity::from_bytes(&self.artifact_bytes),
            MachineRuntimeIdentity {
                machine_id: MachineId::from(PICO8_ID),
                bundle_version: VersionStamp::from(env!("CARGO_PKG_VERSION")),
                machine_version: VersionStamp::from(MACHINE_VERSION),
                emulator_version: VersionStamp::from(EMULATOR_VERSION),
                variant: format_name(self.cartridge.format).into(),
                machine_parameters: BTreeMap::from([
                    ("artifact_version".into(), json!(self.cartridge.version)),
                    ("callback_hz".into(), json!(fps)),
                    (
                        "compatibility_tier".into(),
                        json!("documented-headless-subset"),
                    ),
                    ("instruction_budget".into(), json!(self.instruction_budget)),
                    ("lua_engine".into(), json!("lua54")),
                    ("numeric_model".into(), json!("host-f64")),
                    ("translator".into(), json!("pico8-to-lua/0.1.1")),
                ]),
            },
            InitialStateIdentity::from_bytes(
                InitialStateKind::Boot,
                schema("pico8.state.observable"),
                &self.initial_state_bytes,
            ),
            SeedIdentity {
                requested: self.request.config.seed,
                effective: self.request.config.seed,
                derivation: component_identity(
                    "pico8.seed.direct.v1",
                    "pico8.seed.derivation",
                    json!({}),
                ),
                policy_seed: episode.policy_seed,
                environment_seed: episode.environment_seed,
            },
            component_identity(
                "glassvm.policy.fixed_frames.v1",
                "glassvm.execution_policy",
                json!({"max_frames": self.request.config.max_frames}),
            ),
            episode.interaction_policy.clone(),
            StimulusIdentity::from_stimuli(schema("pico8.input.controller_mask"), &self.stimuli)?,
            episode.body.clone(),
            episode.environment.clone(),
            TimingIdentity {
                mode: component_identity(
                    "pico8.callback_cadence.v1",
                    "pico8.timing",
                    json!({"callback_hz": fps}),
                ),
                cycles_per_tick: Some(self.instruction_budget),
                tick_hz: Some(fps),
            },
            ExecutionBudget {
                max_steps: None,
                max_cycles: None,
                max_ticks: Some(self.request.config.max_frames),
                max_frames: Some(self.request.config.max_frames),
            },
            self.request.observation.clone(),
        ))
    }

    fn apply_stimuli(&mut self, sink: &mut dyn EmissionSink) -> Result<(), String> {
        let frame = self.runtime.frame();
        while let Some(stimulus) = self.stimuli.get(self.next_stimulus) {
            let ordinal = stimulus.ordinal;
            let (stimulus_frame, mask) = decode_stimulus(stimulus)?;
            if stimulus_frame > frame {
                break;
            }
            if stimulus_frame < frame {
                return Err(format!(
                    "PICO-8 stimulus {ordinal} for frame {stimulus_frame} was not applied before frame {frame}"
                ));
            }
            self.runtime.set_input_mask(mask);
            self.next_stimulus += 1;
            self.emit_event(
                EventKind::InputSampled,
                Some(frame),
                Some(json!({"port": "controllers_in", "mask": mask})),
                sink,
            )?;
        }
        Ok(())
    }

    fn apply_stimuli_silently(&mut self) -> Result<(), String> {
        let frame = self.runtime.frame();
        while let Some(stimulus) = self.stimuli.get(self.next_stimulus) {
            let ordinal = stimulus.ordinal;
            let (stimulus_frame, mask) = decode_stimulus(stimulus)?;
            if stimulus_frame > frame {
                break;
            }
            if stimulus_frame < frame {
                return Err(format!(
                    "PICO-8 stimulus {ordinal} for frame {stimulus_frame} was not applied before frame {frame}"
                ));
            }
            self.runtime.set_input_mask(mask);
            self.next_stimulus += 1;
        }
        Ok(())
    }

    fn emit_frame(&mut self, sink: &mut dyn EmissionSink) -> Result<(), String> {
        let pixels = self.runtime.framebuffer();
        let hash = ContentDigest::sha256(&pixels).to_hex();
        self.frame_hashes.push(hash.clone());
        let payload = match self.request.observation.frames {
            FrameCapture::None => None,
            FrameCapture::Hashes => Some(json!({
                "width": DISPLAY_WIDTH,
                "height": DISPLAY_HEIGHT,
                "frame_hash": hash
            })),
            FrameCapture::Full => Some(json!({
                "width": DISPLAY_WIDTH,
                "height": DISPLAY_HEIGHT,
                "planes": 1,
                "palette_indices": pixels,
                "frame_hash": hash
            })),
        };
        self.emit_event(
            EventKind::FrameCompleted,
            self.runtime.frame().checked_sub(1),
            payload,
            sink,
        )?;
        Ok(())
    }

    fn emit_event(
        &mut self,
        kind: EventKind,
        frame: Option<u64>,
        extension: Option<Value>,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        if !self.request.observation.events.includes(&kind) {
            return Ok(());
        }
        let context = EventContext {
            arch: MachineId::from(PICO8_ID),
            machine_version: VersionStamp::from(MACHINE_VERSION),
            run_id: self.request.run_id.clone(),
            sequence: self.next_sequence,
            step: self.runtime.frame(),
            cycle_or_tick: Some(self.runtime.frame()),
            frame,
            pc: None,
            instruction: None,
        };
        let native =
            Pico8NativeEventAdapter::from_execution(&kind, extension.unwrap_or_else(|| json!({})))?;
        let mut normalized = Pico8NativeEventAdapter.normalize(&context, &native)?;
        if normalized.len() != 1 {
            return Err(format!(
                "PICO-8 native adapter produced {} events; expected one",
                normalized.len()
            ));
        }
        let mut event = normalized.remove(0);
        if !self.request.observation.include_extensions
            || !self
                .request
                .observation
                .native_events
                .includes(&native.kind)
        {
            event.extensions.clear();
        }
        self.next_sequence += 1;
        self.trace_fingerprint.update(&event)?;
        sink.emit(Emission::Event(&event)).map_err(sink_error)?;
        if let Some(chunk_size) = self.request.observation.trace_chunk_events {
            self.pending_trace_events.push(event);
            if self.pending_trace_events.len() >= chunk_size {
                self.flush_trace_chunk(sink)?;
            }
        }
        Ok(())
    }

    fn flush_trace_chunk(&mut self, sink: &mut dyn EmissionSink) -> Result<(), String> {
        if self.pending_trace_events.is_empty() {
            return Ok(());
        }
        // The compatibility runtime emits callback/effect boundaries, not a
        // complete PICO-8 VM instruction stream. Never label this as complete.
        let chunk = TraceChunk::new(
            self.next_trace_chunk,
            std::mem::take(&mut self.pending_trace_events),
            true,
            self.previous_trace_chunk_digest,
        )?;
        let digest = chunk.digest()?;
        sink.emit(Emission::TraceChunk(&chunk))
            .map_err(sink_error)?;
        self.previous_trace_chunk_digest = Some(digest);
        self.next_trace_chunk += 1;
        Ok(())
    }

    fn emit_snapshot(&self, sink: &mut dyn EmissionSink) -> Result<(), String> {
        let bytes = self.continuation_bytes()?;
        let snapshot = SnapshotArtifact::new_typed(
            self.request.run_id.clone(),
            MachineId::from(PICO8_ID),
            VersionStamp::from(MACHINE_VERSION),
            schema("pico8.state.continuation"),
            self.next_sequence,
            self.runtime.frame(),
            bytes,
        );
        sink.emit(Emission::Snapshot(&snapshot)).map_err(sink_error)
    }

    fn build_result(
        &self,
        termination: &str,
        runtime_error: Option<&str>,
    ) -> Result<RunResult, String> {
        let snapshot = self.runtime.snapshot();
        let pixels = self.runtime.framebuffer();
        let mut observations = vec![
            NativeObservation {
                schema: schema("pico8.display_dims"),
                name: "pico8.display_dims".into(),
                payload: json!({
                    "width": DISPLAY_WIDTH,
                    "height": DISPLAY_HEIGHT,
                    "planes": 1
                }),
            },
            NativeObservation {
                schema: schema("pico8.framebuffer"),
                name: "pico8.framebuffer_flat".into(),
                payload: json!({
                    "width": DISPLAY_WIDTH,
                    "height": DISPLAY_HEIGHT,
                    "planes": 1,
                    "bytes": pixels
                }),
            },
            NativeObservation {
                schema: schema("pico8.execution_summary"),
                name: "pico8.execution_summary".into(),
                payload: json!({
                    "artifact_format": format_name(self.cartridge.format),
                    "artifact_version": self.cartridge.version,
                    "source_bytes": self.cartridge.lua.len(),
                    "callback_hz": self.runtime.fps(),
                    "frames": snapshot.frame,
                    "draw_calls": snapshot.draw_calls,
                    "audio_calls": snapshot.audio_calls,
                    "print_calls": snapshot.printed.len(),
                    "instruction_budget_per_frame": self.instruction_budget,
                    "compatibility_tier": "documented-headless-subset",
                    "numeric_model": "host-f64"
                }),
            },
            NativeObservation {
                schema: schema("pico8.frame_hashes"),
                name: "pico8.frame_hashes".into(),
                payload: json!(self.frame_hashes),
            },
        ];
        if let Some(error) = runtime_error {
            observations.push(NativeObservation {
                schema: schema("pico8.runtime_error"),
                name: "pico8.runtime_error".into(),
                payload: json!({"message": error}),
            });
        }
        let capabilities = if self.request.observation.collect_summaries {
            Pico8ObservableAdapter.normalize(&observations)?
        } else {
            Vec::new()
        };
        Ok(RunResult {
            common: CommonMetrics {
                cycles: snapshot.frame,
                frames: snapshot.frame,
                termination: termination.into(),
                boot_success: true,
            },
            capabilities,
        })
    }
}

pub(super) fn validate_stimuli(stimuli: &[ReplayStimulus], max_frames: u64) -> Result<(), String> {
    let mut previous_frame = None;
    for (index, stimulus) in stimuli.iter().enumerate() {
        if stimulus.ordinal != index as u64 {
            return Err(format!(
                "PICO-8 stimulus at index {index} has ordinal {}; expected {index}",
                stimulus.ordinal
            ));
        }
        if stimulus.port != "controllers_in" {
            return Err(format!(
                "PICO-8 stimulus {} targets unsupported port {:?}",
                stimulus.ordinal, stimulus.port
            ));
        }
        if stimulus.coordinate.step.is_some() || stimulus.coordinate.cycle_or_tick.is_some() {
            return Err(format!(
                "PICO-8 stimulus {} must use only a frame coordinate",
                stimulus.ordinal
            ));
        }
        let frame = stimulus.coordinate.frame.ok_or_else(|| {
            format!(
                "PICO-8 stimulus {} has no frame coordinate",
                stimulus.ordinal
            )
        })?;
        if frame >= max_frames {
            return Err(format!(
                "PICO-8 stimulus {} targets frame {frame}, outside max_frames {max_frames}",
                stimulus.ordinal
            ));
        }
        if previous_frame.is_some_and(|previous| frame < previous) {
            return Err("PICO-8 stimuli must be ordered by nondecreasing frame".into());
        }
        previous_frame = Some(frame);
        let value = stimulus.value.as_object().ok_or_else(|| {
            format!(
                "PICO-8 stimulus {} value must be an object",
                stimulus.ordinal
            )
        })?;
        if value.len() != 1 || !value.contains_key("mask") {
            return Err(format!(
                "PICO-8 stimulus {} value must contain exactly {{mask}}",
                stimulus.ordinal
            ));
        }
        let valid_mask = stimulus
            .value
            .get("mask")
            .and_then(Value::as_u64)
            .is_some_and(|mask| u16::try_from(mask).is_ok());
        if !valid_mask {
            return Err(format!(
                "PICO-8 stimulus {} must contain a 16-bit numeric mask",
                stimulus.ordinal
            ));
        }
    }
    Ok(())
}

pub(super) fn decode_stimulus(stimulus: &ReplayStimulus) -> Result<(u64, u16), String> {
    let frame = stimulus.coordinate.frame.ok_or_else(|| {
        format!(
            "validated PICO-8 stimulus {} lost its frame coordinate",
            stimulus.ordinal
        )
    })?;
    let mask = stimulus
        .value
        .get("mask")
        .and_then(Value::as_u64)
        .and_then(|mask| u16::try_from(mask).ok())
        .ok_or_else(|| {
            format!(
                "PICO-8 stimulus {} has invalid controller mask",
                stimulus.ordinal
            )
        })?;
    Ok((frame, mask))
}

pub(super) fn apply_replayed_stimuli(
    runtime: &Pico8Runtime,
    stimuli: &[ReplayStimulus],
    applied_stimuli: usize,
    cursor: &mut usize,
) -> Result<(), String> {
    let current_frame = runtime.frame();
    while *cursor < applied_stimuli {
        let stimulus = &stimuli[*cursor];
        let (frame, mask) = decode_stimulus(stimulus)?;
        if frame > current_frame {
            break;
        }
        if frame < current_frame {
            return Err(format!(
                "PICO-8 continuation replay reached frame {current_frame} after missing stimulus {} at frame {frame}",
                stimulus.ordinal
            ));
        }
        runtime.set_input_mask(mask);
        *cursor += 1;
    }
    Ok(())
}
