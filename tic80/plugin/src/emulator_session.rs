use glassvm_core::{
    Address, CausalLink, CausalRelation, CommonMetrics, ContentDigest, Emission, EmissionSink,
    EmulatorSession, EventContext, EventKind, ExecutionEvent, ExecutionRequest, FrameCapture,
    IoChannel, IoDirection, IoObservation, MachineId, NativeEventAdapter, NativeObservation,
    ObservableAdapter, RunResult, SnapshotArtifact, SnapshotCapture, TraceChunk, VersionStamp,
};
use serde_json::{Value, json};
use tic80_core::{HEIGHT, Tic80Event, Tic80Runtime, WIDTH};

use crate::adapters::{Tic80NativeEventAdapter, Tic80ObservableAdapter};
use crate::identity::{
    MACHINE_ID, REPLAY_SNAPSHOT_FORMAT_VERSION, SEMANTICS, replay_snapshot_schema,
};
use crate::session_snapshot::{ReplaySnapshot, SessionLifecycle, snapshot_digest};
use crate::trace::sha256_hex;

pub(super) struct Tic80Session {
    pub(super) request: ExecutionRequest,
    pub(super) runtime: Tic80Runtime,
    pub(super) input_mask: u32,
    pub(super) input_override_pending: bool,
    pub(super) next_stimulus: usize,
    pub(super) lifecycle: SessionLifecycle,
}

impl Tic80Session {
    fn ensure_not_terminal(&self, action: &str) -> Result<(), String> {
        if self.lifecycle == SessionLifecycle::Terminal {
            return Err(format!("TIC-80 session is terminal; reset before {action}"));
        }
        Ok(())
    }

    fn begin_execute(&mut self) -> Result<(), String> {
        match self.lifecycle {
            SessionLifecycle::Fresh => {
                self.lifecycle = SessionLifecycle::Terminal;
                Ok(())
            }
            SessionLifecycle::Incremental => Err(
                "TIC-80 execute() cannot follow incremental stepping or snapshot restoration; reset first"
                    .into(),
            ),
            SessionLifecycle::Terminal => {
                Err("TIC-80 session is terminal; reset before execute()".into())
            }
        }
    }

    fn apply_stimuli_for_current_frame(&mut self) -> Result<(), String> {
        let frame = self.runtime.input_history().len() as u64;
        while let Some(stimulus) = self.request.stimuli.get(self.next_stimulus) {
            let stimulus_frame = stimulus
                .coordinate
                .frame
                .expect("validated frame coordinate");
            if stimulus_frame > frame {
                break;
            }
            if stimulus_frame < frame {
                return Err(format!(
                    "TIC-80 stimulus {} at frame {stimulus_frame} was not applied before frame {frame}",
                    stimulus.ordinal
                ));
            }
            self.input_mask = stimulus.value["mask"]
                .as_u64()
                .and_then(|mask| u32::try_from(mask).ok())
                .expect("validated gamepad mask");
            self.input_override_pending = false;
            self.next_stimulus += 1;
        }
        Ok(())
    }

    fn event_context(&self, sequence: u64, frame: u64) -> EventContext {
        EventContext {
            arch: MachineId::from(MACHINE_ID),
            machine_version: VersionStamp::from(SEMANTICS),
            run_id: self.request.run_id.clone(),
            sequence,
            step: frame,
            cycle_or_tick: Some(frame),
            frame: Some(frame),
            pc: Some(Address::new("lua_frame", frame)),
            instruction: None,
        }
    }

    fn emit_lifecycle_event(
        &self,
        sink: &mut dyn EmissionSink,
        sequence: u64,
        kind: EventKind,
        io: Vec<IoObservation>,
        trace_events: &mut Vec<ExecutionEvent>,
    ) -> Result<(), String> {
        if !self.request.observation.events.includes(&kind) {
            return Ok(());
        }
        let state = self.runtime.snapshot()?;
        let mut event =
            ExecutionEvent::from_context(&self.event_context(sequence, state.frame), kind);
        event.io = io;
        sink.emit(Emission::Event(&event))
            .map_err(|error| format!("TIC-80 emission sink failed: {error}"))?;
        trace_events.push(event);
        Ok(())
    }

    fn emit_native_event(
        &self,
        sink: &mut dyn EmissionSink,
        sequence: u64,
        frame: u64,
        event: &Tic80Event,
        input_cause: Option<u64>,
        trace_events: &mut Vec<ExecutionEvent>,
    ) -> Result<bool, String> {
        let native = Tic80NativeEventAdapter::from_core(event);
        let io = match event {
            Tic80Event::InputSampled { mask } => vec![IoObservation {
                port: "gamepad_in".into(),
                direction: IoDirection::Input,
                channel: IoChannel::Keypad,
                value: json!({"mask": mask}),
            }],
            Tic80Event::FrameCompleted { .. } => {
                let framebuffer = self.runtime.framebuffer()?;
                vec![IoObservation {
                    port: "display_out".into(),
                    direction: IoDirection::Output,
                    channel: IoChannel::Display,
                    value: json!({
                        "width": framebuffer.width,
                        "height": framebuffer.height,
                        "sha256": sha256_hex(&framebuffer.rgba),
                    }),
                }]
            }
            Tic80Event::Trace { .. } => Vec::new(),
        };
        let mut emitted = false;
        for mut normalized in
            Tic80NativeEventAdapter.normalize(&self.event_context(sequence, frame), &native)?
        {
            if self.request.observation.events.includes(&normalized.kind) {
                if !self.request.observation.include_extensions
                    || !self
                        .request
                        .observation
                        .native_events
                        .includes(&native.kind)
                {
                    normalized.extensions.clear();
                }
                normalized.io = io.clone();
                if !matches!(event, Tic80Event::InputSampled { .. })
                    && let Some(source_sequence) = input_cause
                {
                    normalized.causes.push(CausalLink {
                        source_sequence,
                        relation: CausalRelation::Input,
                        evidence: Some("TIC-80 frame uses the sampled gamepad mask".into()),
                    });
                }
                sink.emit(Emission::Event(&normalized))
                    .map_err(|error| format!("TIC-80 emission sink failed: {error}"))?;
                trace_events.push(normalized);
                emitted = true;
            }
        }
        Ok(emitted)
    }

    fn emit_trace_chunks(
        &self,
        sink: &mut dyn EmissionSink,
        events: &[ExecutionEvent],
    ) -> Result<(), String> {
        let Some(max_events) = self.request.observation.trace_chunk_events else {
            return Ok(());
        };
        let filtered = !self.request.observation.produces_complete_trace();
        let mut previous_chunk_digest = None;
        for (chunk_index, events) in events.chunks(max_events).enumerate() {
            let chunk = TraceChunk::new(
                chunk_index as u64,
                events.to_vec(),
                filtered,
                previous_chunk_digest,
            )?;
            previous_chunk_digest = Some(chunk.digest()?);
            sink.emit(Emission::TraceChunk(&chunk))
                .map_err(|error| format!("TIC-80 emission sink failed: {error}"))?;
        }
        Ok(())
    }

    fn snapshot_bytes_for(&self, lifecycle: SessionLifecycle) -> Result<Vec<u8>, String> {
        let mut snapshot = ReplaySnapshot {
            version: REPLAY_SNAPSHOT_FORMAT_VERSION,
            cart_sha256: sha256_hex(self.runtime.artifact_bytes()),
            request_sha256: sha256_hex(&glassvm_core::canonical_json_bytes(&self.request)?),
            input_history: self.runtime.input_history().to_vec(),
            input_mask: self.input_mask,
            input_override_pending: self.input_override_pending,
            next_stimulus: self.next_stimulus,
            lifecycle,
            payload_digest: ContentDigest::sha256(&[]),
        };
        snapshot.payload_digest = snapshot_digest(&snapshot)?;
        serde_json::to_vec(&snapshot).map_err(|error| error.to_string())
    }

    fn snapshot_bytes(&self) -> Result<Vec<u8>, String> {
        self.snapshot_bytes_for(self.lifecycle)
    }

    fn emit_snapshot(
        &self,
        sink: &mut dyn EmissionSink,
        sequence: u64,
        lifecycle: SessionLifecycle,
    ) -> Result<(), String> {
        let snapshot = SnapshotArtifact::new_typed(
            self.request.run_id.clone(),
            MachineId::from(MACHINE_ID),
            VersionStamp::from(SEMANTICS),
            replay_snapshot_schema(),
            sequence,
            self.runtime.input_history().len() as u64,
            self.snapshot_bytes_for(lifecycle)?,
        );
        sink.emit(Emission::Snapshot(&snapshot))
            .map_err(|error| format!("TIC-80 emission sink failed: {error}"))
    }
}

impl EmulatorSession for Tic80Session {
    fn request(&self) -> &ExecutionRequest {
        &self.request
    }

    fn execute(&mut self, sink: &mut dyn EmissionSink) -> Result<RunResult, String> {
        self.begin_execute()?;
        sink.emit(Emission::RunStarted(&self.request))
            .map_err(|error| format!("TIC-80 emission sink failed: {error}"))?;
        let mut trace_events = Vec::new();
        self.emit_lifecycle_event(
            sink,
            0,
            EventKind::RunStarted,
            Vec::new(),
            &mut trace_events,
        )?;

        let mut sequence = 0;
        for _ in 0..self.request.config.max_frames {
            self.apply_stimuli_for_current_frame()?;
            let outcome = self.runtime.tick(self.input_mask)?;
            self.input_override_pending = false;
            let mut input_sequence = None;
            for event in &outcome.events {
                let event_sequence = sequence + 1;
                let emitted = self.emit_native_event(
                    sink,
                    event_sequence,
                    outcome.frame,
                    event,
                    input_sequence,
                    &mut trace_events,
                )?;
                if emitted {
                    sequence = event_sequence;
                    if matches!(event, Tic80Event::InputSampled { .. }) {
                        input_sequence = Some(sequence);
                    }
                }
            }
            let completed_frames = self.runtime.input_history().len() as u64;
            if let SnapshotCapture::EverySteps(interval) = self.request.observation.snapshots
                && completed_frames.is_multiple_of(interval)
            {
                let lifecycle = if outcome.exit_requested
                    || completed_frames >= self.request.config.max_frames
                {
                    SessionLifecycle::Terminal
                } else {
                    SessionLifecycle::Incremental
                };
                self.emit_snapshot(sink, sequence + 1, lifecycle)?;
            }
            if outcome.exit_requested {
                break;
            }
        }

        let state = self.runtime.snapshot()?;
        let rgba = self.runtime.framebuffer()?.rgba;
        let mut capabilities = if self.request.observation.collect_summaries {
            Tic80ObservableAdapter.normalize(&[
                NativeObservation {
                    schema: glassvm_core::SchemaRef::new(
                        "tic80.framebuffer",
                        glassvm_core::SchemaVersion::V1,
                    ),
                    name: "framebuffer_rgba".into(),
                    payload: json!({
                        "width": WIDTH,
                        "height": HEIGHT,
                        "sha256": sha256_hex(&rgba),
                        "bytes": if matches!(self.request.observation.frames, FrameCapture::Full) {
                            json!(rgba)
                        } else {
                            Value::Null
                        },
                    }),
                },
                NativeObservation {
                    schema: glassvm_core::SchemaRef::new(
                        "tic80.execution",
                        glassvm_core::SchemaVersion::V1,
                    ),
                    name: "frame_summary".into(),
                    payload: json!({
                        "frames": state.frame,
                        "trace_count": state.traces.len(),
                        "traces": state.traces,
                        "language": "lua",
                        "runtime": "lua54",
                    }),
                },
            ])?
        } else {
            Vec::new()
        };
        if self.request.observation.collect_summaries {
            for capability in &capabilities {
                sink.emit(Emission::Capability(capability))
                    .map_err(|error| format!("TIC-80 emission sink failed: {error}"))?;
            }
        }

        let lifecycle_sequence = sequence + 1;
        if matches!(self.request.observation.snapshots, SnapshotCapture::Final) {
            self.emit_snapshot(sink, lifecycle_sequence, SessionLifecycle::Terminal)?;
        }

        let state = self.runtime.snapshot()?;
        let frames = state.frame;
        let termination = if state.exit_requested {
            "ExitRequested"
        } else {
            "FrameBudget"
        };
        let result = RunResult {
            common: CommonMetrics {
                cycles: frames,
                frames,
                termination: termination.into(),
                boot_success: true,
            },
            capabilities: std::mem::take(&mut capabilities),
        };
        self.emit_lifecycle_event(
            sink,
            lifecycle_sequence,
            EventKind::RunHalted,
            Vec::new(),
            &mut trace_events,
        )?;
        self.emit_trace_chunks(sink, &trace_events)?;
        sink.emit(Emission::RunFinished(&result))
            .map_err(|error| format!("TIC-80 emission sink failed: {error}"))?;
        Ok(result)
    }

    fn step_frame(&mut self) -> Result<(), String> {
        self.ensure_not_terminal("step_frame()")?;
        let completed_frames = self.runtime.input_history().len() as u64;
        if completed_frames >= self.request.config.max_frames {
            return Err(format!(
                "TIC-80 step_frame() would exceed frozen max_frames {}",
                self.request.config.max_frames
            ));
        }
        self.lifecycle = SessionLifecycle::Incremental;
        let outcome = (|| {
            self.apply_stimuli_for_current_frame()?;
            self.runtime.tick(self.input_mask)?;
            self.input_override_pending = false;
            Ok(())
        })();
        if outcome.is_err() {
            self.lifecycle = SessionLifecycle::Terminal;
        }
        outcome
    }

    fn reset(&mut self) -> Result<(), String> {
        self.runtime.reset()?;
        self.input_mask = 0;
        self.input_override_pending = false;
        self.next_stimulus = 0;
        self.lifecycle = SessionLifecycle::Fresh;
        Ok(())
    }

    fn snapshot(&self) -> Result<Vec<u8>, String> {
        self.snapshot_bytes()
    }

    fn restore_snapshot(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.ensure_not_terminal("restore_snapshot()")?;
        let snapshot: ReplaySnapshot =
            serde_json::from_slice(bytes).map_err(|error| format!("invalid snapshot: {error}"))?;
        if snapshot.payload_digest != snapshot_digest(&snapshot)? {
            return Err("TIC-80 snapshot payload digest mismatch".into());
        }
        if snapshot.version != REPLAY_SNAPSHOT_FORMAT_VERSION {
            return Err(format!(
                "unsupported TIC-80 snapshot version {}; expected {}",
                snapshot.version, REPLAY_SNAPSHOT_FORMAT_VERSION
            ));
        }
        let actual = sha256_hex(self.runtime.artifact_bytes());
        if snapshot.cart_sha256 != actual {
            return Err("TIC-80 snapshot cartridge digest mismatch".into());
        }
        let request_sha256 = sha256_hex(&glassvm_core::canonical_json_bytes(&self.request)?);
        if snapshot.request_sha256 != request_sha256 {
            return Err("TIC-80 snapshot execution-request identity mismatch".into());
        }
        if snapshot.next_stimulus > self.request.stimuli.len() {
            return Err("TIC-80 snapshot stimulus cursor exceeds request stimuli".into());
        }
        if snapshot.input_history.len() as u64 > self.request.config.max_frames {
            return Err(format!(
                "TIC-80 snapshot contains {} frames, exceeding frozen max_frames {}",
                snapshot.input_history.len(),
                self.request.config.max_frames
            ));
        }
        let frame = snapshot.input_history.len() as u64;
        for (index, stimulus) in self.request.stimuli.iter().enumerate() {
            let should_be_consumed = stimulus.coordinate.frame.expect("validated") < frame;
            if (index < snapshot.next_stimulus) != should_be_consumed {
                return Err(
                    "TIC-80 snapshot stimulus cursor is inconsistent with its input history".into(),
                );
            }
        }
        if snapshot.input_override_pending && snapshot.next_stimulus < self.request.stimuli.len() {
            return Err(
                "TIC-80 snapshot cannot have immediate input pending while scheduled stimuli remain"
                    .into(),
            );
        }
        if snapshot.input_override_pending && frame >= self.request.config.max_frames {
            return Err(
                "TIC-80 snapshot cannot carry pending immediate input beyond the frame budget"
                    .into(),
            );
        }
        if let Some(last_scheduled_frame) = self
            .request
            .stimuli
            .last()
            .and_then(|stimulus| stimulus.coordinate.frame)
        {
            let mut stimulus_index = 0usize;
            let mut scheduled_mask = 0u32;
            for (frame_index, actual_mask) in snapshot.input_history.iter().enumerate() {
                let replay_frame = frame_index as u64;
                if replay_frame > last_scheduled_frame {
                    break;
                }
                while let Some(stimulus) = self.request.stimuli.get(stimulus_index) {
                    if stimulus.coordinate.frame.expect("validated") != replay_frame {
                        break;
                    }
                    scheduled_mask = stimulus.value["mask"]
                        .as_u64()
                        .and_then(|mask| u32::try_from(mask).ok())
                        .expect("validated gamepad mask");
                    stimulus_index += 1;
                }
                if *actual_mask != scheduled_mask {
                    return Err(format!(
                        "TIC-80 snapshot input history at frame {replay_frame} has mask {actual_mask:#x}; scheduled replay requires {scheduled_mask:#x}"
                    ));
                }
            }
        }
        let completed_input = snapshot.input_history.last().copied().unwrap_or(0);
        if !snapshot.input_override_pending && snapshot.input_mask != completed_input {
            return Err(
                "TIC-80 snapshot persistent input mask disagrees with its input history".into(),
            );
        }
        if snapshot.lifecycle == SessionLifecycle::Fresh
            && (!snapshot.input_history.is_empty()
                || snapshot.input_mask != 0
                || snapshot.input_override_pending
                || snapshot.next_stimulus != 0)
        {
            return Err("TIC-80 fresh snapshot does not contain the exact reset state".into());
        }
        if self.lifecycle == SessionLifecycle::Incremental {
            if snapshot.lifecycle == SessionLifecycle::Fresh {
                return Err(
                    "TIC-80 snapshot restore would rewind an incremental session to fresh".into(),
                );
            }
            if snapshot.input_history.len() < self.runtime.input_history().len()
                || !snapshot
                    .input_history
                    .starts_with(self.runtime.input_history())
            {
                return Err(
                    "TIC-80 snapshot restore is stale or leaves the current replay line".into(),
                );
            }
        }
        self.runtime.restore_history(&snapshot.input_history)?;
        self.input_mask = snapshot.input_mask;
        self.input_override_pending = snapshot.input_override_pending;
        self.next_stimulus = snapshot.next_stimulus;
        self.lifecycle = snapshot.lifecycle;
        Ok(())
    }

    fn set_input_mask(&mut self, mask: u64) -> Result<(), String> {
        self.ensure_not_terminal("set_input_mask()")?;
        if mask > u32::MAX as u64 {
            return Err("TIC-80 input mask exceeds four packed gamepads".into());
        }
        if self.next_stimulus < self.request.stimuli.len() {
            return Err(
                "TIC-80 immediate input cannot overwrite unconsumed scheduled stimuli".into(),
            );
        }
        let completed_frames = self.runtime.input_history().len() as u64;
        if completed_frames >= self.request.config.max_frames {
            return Err(format!(
                "TIC-80 set_input_mask() is outside frozen max_frames {}",
                self.request.config.max_frames
            ));
        }
        self.input_mask = mask as u32;
        self.input_override_pending = true;
        self.lifecycle = SessionLifecycle::Incremental;
        Ok(())
    }
}
