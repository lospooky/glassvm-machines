use glassvm_core::{
    CommonMetrics, ContentDigest, Emission, EmissionSink, EmulatorSession, EventContext, EventKind,
    ExecutionEvent, ExecutionRequest, FrameArtifact, FrameCapture, FrameEvidence, InputCoordinate,
    InputId, InputSource, InputValueEvidence, MachineId, NativeEvent, NativeEvidenceEnvelope,
    PreparedObservation, PreparedRun, RunResult, SinkError, SnapshotArtifact, SnapshotCapture,
    StructuredValue, TypedInputPayload, VersionStamp, canonical_json_fingerprint,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tic80_core::{RuntimeSnapshot, Tic80Runtime};

use crate::emulator_backend::configuration_values;

#[derive(Clone)]
struct ScheduledInput {
    ordinal: u64,
    frame: u64,
    input_id: InputId,
    payload: TypedInputPayload,
    mask: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tic80MachineState {
    ram: Vec<u8>,
    overlay_vram: Vec<u8>,
    active_video_bank: u8,
    frame: u64,
    input: u32,
    previous_input: u32,
    button_holds: [u32; 32],
    clip: [i32; 4],
    exit_requested: bool,
}

impl Tic80MachineState {
    fn from_native(native: &RuntimeSnapshot) -> Self {
        Self {
            ram: native.ram.clone(),
            overlay_vram: native.overlay_vram.clone(),
            active_video_bank: native.active_video_bank,
            frame: native.frame,
            input: native.input,
            previous_input: native.previous_input,
            button_holds: native.button_holds,
            clip: native.clip,
            exit_requested: native.exit_requested,
        }
    }

    fn to_native(&self) -> RuntimeSnapshot {
        RuntimeSnapshot {
            ram: self.ram.clone(),
            overlay_vram: self.overlay_vram.clone(),
            active_video_bank: self.active_video_bank,
            frame: self.frame,
            input: self.input,
            previous_input: self.previous_input,
            button_holds: self.button_holds,
            clip: self.clip,
            traces: Vec::new(),
            exit_requested: self.exit_requested,
            input_history: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tic80SessionSnapshot {
    format: String,
    version: u32,
    request_digest: ContentDigest,
    machine_state: Tic80MachineState,
    next_input: usize,
}

pub(super) struct Tic80Session {
    request: ExecutionRequest,
    artifact: Vec<u8>,
    runtime: Tic80Runtime,
    scheduled_inputs: Vec<ScheduledInput>,
    next_input: usize,
    cycles_per_frame: u32,
    machine_seed: u64,
    prepared_observation: PreparedObservation,
    next_sequence: u64,
    terminal: bool,
}

impl Tic80Session {
    fn current_frame(&self) -> Result<u64, String> {
        Ok(self.runtime.snapshot()?.frame)
    }

    pub(super) fn new(
        artifact: &[u8],
        request: ExecutionRequest,
        prepared_run: PreparedRun,
        prepared_observation: PreparedObservation,
    ) -> Result<Self, String> {
        let (cycles_per_frame, machine_seed) = configuration_values(&prepared_run.configuration)?;
        let runtime = Tic80Runtime::new(artifact, machine_seed)?;
        let scheduled_inputs = scheduled_inputs(&prepared_run)?;
        Ok(Self {
            request,
            artifact: artifact.to_vec(),
            runtime,
            scheduled_inputs,
            next_input: 0,
            cycles_per_frame,
            machine_seed,
            prepared_observation,
            next_sequence: 0,
            terminal: false,
        })
    }

    fn frame_limit(&self) -> Result<u64, String> {
        self.request
            .execution_controls
            .frame_limit
            .ok_or_else(|| "TIC-80 execution requires an explicit frame_limit".into())
    }

    fn input_for_frame(&mut self, frame: u64, sink: &mut dyn EmissionSink) -> Result<u32, String> {
        let mut mask = 0;
        while let Some(input) = self.scheduled_inputs.get(self.next_input).cloned() {
            if input.frame > frame {
                break;
            }
            if input.frame < frame {
                return Err(format!(
                    "TIC-80 scheduled input for frame {} was not applied before frame {frame}",
                    input.frame
                ));
            }
            mask = input.mask;
            self.emit_input_applied(&input, frame, mask, sink)?;
            self.next_input += 1;
        }
        Ok(mask)
    }

    fn step_native_frame(&mut self) -> Result<bool, String> {
        let frame = self.current_frame()?;
        let mask = self.input_for_frame(frame, &mut glassvm_core::NullSink)?;
        Ok(self.runtime.tick(mask)?.exit_requested)
    }

    fn reset_runtime(&mut self) -> Result<(), String> {
        self.runtime = Tic80Runtime::new(&self.artifact, self.machine_seed)?;
        self.next_input = 0;
        self.next_sequence = 0;
        self.terminal = false;
        Ok(())
    }

    fn request_digest(&self) -> Result<ContentDigest, String> {
        canonical_json_fingerprint("tic80.session_request", &self.request)
    }

    fn snapshot_bytes(&self) -> Result<Vec<u8>, String> {
        let snapshot = Tic80SessionSnapshot {
            format: crate::identity::SESSION_SNAPSHOT_FORMAT.into(),
            version: crate::identity::SESSION_SNAPSHOT_FORMAT_VERSION,
            request_digest: self.request_digest()?,
            machine_state: Tic80MachineState::from_native(&self.runtime.snapshot()?),
            next_input: self.next_input,
        };
        serde_json::to_vec(&snapshot)
            .map_err(|error| format!("TIC-80 session snapshot serialization failed: {error}"))
    }

    fn emit_snapshot(&mut self, sink: &mut dyn EmissionSink) -> Result<SnapshotArtifact, String> {
        let step = self.current_frame()?;
        let snapshot = SnapshotArtifact::new_typed(
            self.request.run_id.clone(),
            MachineId::from(tic80_core::MACHINE_ID),
            VersionStamp::from(tic80_core::SEMANTICS),
            crate::identity::session_snapshot_schema(),
            self.next_sequence,
            step,
            self.snapshot_bytes()?,
        );
        sink.emit(Emission::Snapshot(&snapshot))
            .map_err(sink_error)?;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| "TIC-80 emission sequence counter exhausted u64".to_string())?;
        Ok(snapshot)
    }

    fn replay_inputs_for_snapshot(
        &self,
        frame_count: u64,
        next_input: usize,
    ) -> Result<Vec<u32>, String> {
        let mut cursor = 0usize;
        let mut inputs = Vec::with_capacity(
            usize::try_from(frame_count)
                .map_err(|_| "TIC-80 session snapshot frame count is too large")?,
        );
        for frame in 0..frame_count {
            let mut mask = 0;
            while cursor < next_input {
                let input = self
                    .scheduled_inputs
                    .get(cursor)
                    .ok_or_else(|| "TIC-80 session snapshot input cursor is invalid".to_string())?;
                if input.frame < frame {
                    return Err(
                        "TIC-80 session snapshot schedule cursor skips an input frame".into(),
                    );
                }
                if input.frame > frame {
                    break;
                }
                mask = input.mask;
                cursor += 1;
            }
            inputs.push(mask);
        }
        if cursor != next_input {
            return Err(
                "TIC-80 session snapshot input cursor is ahead of its machine frame".into(),
            );
        }
        Ok(inputs)
    }
}

impl EmulatorSession for Tic80Session {
    fn request(&self) -> &ExecutionRequest {
        &self.request
    }

    fn execute(&mut self, sink: &mut dyn EmissionSink) -> Result<RunResult, String> {
        if self.terminal {
            return Err("TIC-80 session is terminal; reset before execute".into());
        }
        self.prepared_observation.validate()?;
        let frame_limit = self.frame_limit()?;
        let normalizer = crate::normalizer::Tic80Normalizer::new();
        let mut normalizer_driver = glassvm_core::NormalizerDriver::new_with_prepared_observation(
            normalizer,
            sink,
            &self.prepared_observation,
        );
        let sink: &mut dyn EmissionSink = &mut normalizer_driver;
        self.next_sequence = 0;
        sink.emit(Emission::RunStarted(&self.request))
            .map_err(sink_error)?;
        self.emit_normalized(EventKind::RunStarted, None, 0, sink)?;
        let mut exit_requested = false;
        let mut final_state = None;
        while self.current_frame()? < frame_limit {
            let frame = self.current_frame()?;
            let mask = self.input_for_frame(frame, sink)?;
            let outcome = self.runtime.tick(mask)?;
            let completed_frame = outcome.frame.saturating_sub(1);
            for native_event in &outcome.events {
                self.emit_native_event(native_event, completed_frame, sink)?;
                self.emit_normalized_event(native_event, completed_frame, sink)?;
            }
            self.emit_frame_artifact(completed_frame, sink)?;
            if let SnapshotCapture::EverySteps(interval) =
                self.request.observation.snapshots.capture
                && interval != 0
                && self.current_frame()?.is_multiple_of(interval)
            {
                final_state = Some(self.emit_snapshot(sink)?);
            }
            exit_requested = outcome.exit_requested;
            if exit_requested {
                break;
            }
        }
        if matches!(
            self.request.observation.snapshots.capture,
            SnapshotCapture::Final
        ) {
            final_state = Some(self.emit_snapshot(sink)?);
        }
        self.terminal = true;
        let frames = self.current_frame()?;
        self.emit_normalized(EventKind::RunHalted, Some(frames), frames, sink)?;
        let snapshot = self.runtime.snapshot()?;
        self.emit_native(
            "tic80.execution_summary",
            json!({
                "frames": frames,
                "trace_events": snapshot.traces.len(),
                "exit_requested": exit_requested,
                "runtime": "lua",
            }),
            frames,
            sink,
        )?;
        let mut result = RunResult {
            common: CommonMetrics {
                cycles: frames.saturating_mul(u64::from(self.cycles_per_frame)),
                frames,
                termination: if exit_requested {
                    "machine_exit".into()
                } else {
                    "frame_limit".into()
                },
                boot_success: true,
            },
            capabilities: Vec::new(),
        };
        let normalizer_run = normalizer_driver
            .finish(final_state.as_ref())
            .map_err(|error| error.to_string())?;
        let (downstream, normalizer_output) = normalizer_run.into_parts();
        result.capabilities = normalizer_output.capabilities;
        downstream
            .emit(Emission::RunFinished(&result))
            .map_err(sink_error)?;
        Ok(result)
    }

    fn step_frame(&mut self) -> Result<(), String> {
        if self.terminal {
            return Err("TIC-80 session is terminal; reset before stepping".into());
        }
        self.step_native_frame().map(|_| ())
    }

    fn reset(&mut self) -> Result<(), String> {
        self.reset_runtime()
    }

    fn snapshot(&self) -> Result<Vec<u8>, String> {
        self.snapshot_bytes()
    }

    fn restore_snapshot(&mut self, bytes: &[u8]) -> Result<(), String> {
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid TIC-80 session snapshot: {error}"))?;
        let version = value.get("version").and_then(serde_json::Value::as_u64);
        if version != Some(u64::from(crate::identity::SESSION_SNAPSHOT_FORMAT_VERSION)) {
            return Err(format!(
                "unsupported TIC-80 session snapshot version {:?}; restart the run with snapshot format v{}",
                version,
                crate::identity::SESSION_SNAPSHOT_FORMAT_VERSION
            ));
        }
        let snapshot: Tic80SessionSnapshot = serde_json::from_value(value)
            .map_err(|error| format!("invalid TIC-80 session snapshot: {error}"))?;
        if snapshot.format != crate::identity::SESSION_SNAPSHOT_FORMAT {
            return Err(format!(
                "unsupported TIC-80 session snapshot format {:?}",
                snapshot.format
            ));
        }
        if snapshot.request_digest != self.request_digest()? {
            return Err("TIC-80 session snapshot execution identity mismatch".into());
        }
        if snapshot.next_input > self.scheduled_inputs.len() {
            return Err("TIC-80 session snapshot input cursor exceeds its schedule".into());
        }
        let replay_inputs =
            self.replay_inputs_for_snapshot(snapshot.machine_state.frame, snapshot.next_input)?;
        self.runtime.restore_machine_state_from_inputs(
            &snapshot.machine_state.to_native(),
            &replay_inputs,
        )?;
        self.next_input = snapshot.next_input;
        self.next_sequence = 0;
        self.terminal = false;
        Ok(())
    }
}

impl Tic80Session {
    fn emit_input_applied(
        &mut self,
        input: &ScheduledInput,
        frame: u64,
        mask: u32,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        let mut event = self.event(EventKind::InputApplied, Some(frame), frame);
        event.extensions.insert(
            "tic80.input".into(),
            json!({"input_id": input.input_id, "ordinal": input.ordinal}),
        );
        if self
            .request
            .observation
            .normalized_events
            .events
            .includes(&EventKind::InputApplied)
        {
            self.emit_selected_event(event, sink)?;
        }
        self.emit_native(
            "tic80.input_applied",
            json!({"input_id": input.input_id, "ordinal": input.ordinal, "mask": mask}),
            frame,
            sink,
        )?;
        if self
            .prepared_observation
            .input_value_ids
            .iter()
            .any(|id| id == &input.input_id)
        {
            let evidence = InputValueEvidence::new(
                input.input_id.clone(),
                input.payload.schema.clone(),
                InputSource::Scheduled {
                    ordinal: input.ordinal,
                },
                InputCoordinate::frame(frame),
                input.payload.clone(),
            );
            sink.emit(Emission::InputValueEvidence(&evidence))
                .map_err(sink_error)?;
        }
        Ok(())
    }

    fn emit_native_event(
        &mut self,
        native_event: &tic80_core::Tic80Event,
        frame: u64,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        let (kind, payload) = match native_event {
            tic80_core::Tic80Event::FrameCompleted { frame } => (
                "tic80.frame_completed",
                json!({"frame": frame.saturating_sub(1)}),
            ),
            tic80_core::Tic80Event::InputSampled { mask } => {
                ("tic80.input_sampled", json!({"mask": mask}))
            }
            tic80_core::Tic80Event::Trace { message } => {
                ("tic80.trace", json!({"message": message}))
            }
        };
        self.emit_native(kind, payload, frame, sink)
    }

    fn emit_normalized_event(
        &mut self,
        native_event: &tic80_core::Tic80Event,
        frame: u64,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        let (kind, extension) = match native_event {
            tic80_core::Tic80Event::FrameCompleted { .. } => (EventKind::FrameCompleted, None),
            tic80_core::Tic80Event::InputSampled { mask } => {
                (EventKind::InputSampled, Some(json!({"mask": mask})))
            }
            tic80_core::Tic80Event::Trace { message } => (
                EventKind::Extension("tic80.trace".into()),
                Some(json!({"message": message})),
            ),
        };
        if !self
            .request
            .observation
            .normalized_events
            .events
            .includes(&kind)
        {
            return Ok(());
        }
        let mut event = self.event(kind, Some(frame), frame);
        if let Some(value) = extension {
            event.extensions.insert("tic80.native".into(), value);
        }
        if let tic80_core::Tic80Event::InputSampled { mask } = native_event {
            event.io.push(glassvm_core::IoObservation {
                port: "gamepad_in".into(),
                direction: glassvm_core::IoDirection::Input,
                channel: glassvm_core::IoChannel::Keypad,
                value: json!({"mask": mask}),
            });
        }
        self.emit_selected_event(event, sink)
    }

    fn emit_frame_artifact(
        &mut self,
        frame: u64,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        let framebuffer = self.runtime.framebuffer()?;
        let evidence = match self.request.observation.frames.capture {
            FrameCapture::None => return Ok(()),
            FrameCapture::Hashes => FrameEvidence::Fingerprint {
                bytes: ContentDigest::sha256(&framebuffer.rgba).value.to_vec(),
            },
            FrameCapture::Full => FrameEvidence::Full {
                bytes: framebuffer.rgba,
            },
        };
        let artifact = match evidence {
            FrameEvidence::Fingerprint { bytes } => FrameArtifact::fingerprint(
                self.request.run_id.clone(),
                MachineId::from(tic80_core::MACHINE_ID),
                VersionStamp::from(tic80_core::SEMANTICS),
                glassvm_core::SchemaRef::new(
                    "tic80.frame.rgba8.sha256",
                    glassvm_core::SchemaVersion::V1,
                ),
                self.next_sequence,
                frame,
                frame,
                bytes,
            ),
            FrameEvidence::Full { bytes } => FrameArtifact::full(
                self.request.run_id.clone(),
                MachineId::from(tic80_core::MACHINE_ID),
                VersionStamp::from(tic80_core::SEMANTICS),
                glassvm_core::SchemaRef::new("tic80.frame.rgba8", glassvm_core::SchemaVersion::V1),
                self.next_sequence,
                frame,
                frame,
                bytes,
            ),
        };
        sink.emit(Emission::Frame(&artifact)).map_err(sink_error)?;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| "TIC-80 emission sequence counter exhausted u64".to_string())?;
        Ok(())
    }

    fn emit_normalized(
        &mut self,
        kind: EventKind,
        frame: Option<u64>,
        step: u64,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        if !self
            .request
            .observation
            .normalized_events
            .events
            .includes(&kind)
        {
            return Ok(());
        }
        self.emit_selected_event(self.event(kind, frame, step), sink)
    }

    fn emit_selected_event(
        &mut self,
        event: ExecutionEvent,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        sink.emit(Emission::Event(&event)).map_err(sink_error)?;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| "TIC-80 emission sequence counter exhausted u64".to_string())?;
        Ok(())
    }

    fn emit_native(
        &mut self,
        kind: &str,
        payload: serde_json::Value,
        step: u64,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        if !self.request.observation.native_evidence.includes(kind) {
            return Ok(());
        }
        let evidence = NativeEvidenceEnvelope::new(
            self.request.run_id.clone(),
            MachineId::from(tic80_core::MACHINE_ID),
            VersionStamp::from(tic80_core::SEMANTICS),
            self.next_sequence,
            step,
            NativeEvent {
                schema: glassvm_core::SchemaRef::new(
                    "tic80.native_event",
                    glassvm_core::SchemaVersion::V1,
                ),
                kind: kind.into(),
                payload,
            },
        );
        sink.emit(Emission::NativeEvidence(&evidence))
            .map_err(sink_error)?;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| "TIC-80 emission sequence counter exhausted u64".to_string())?;
        Ok(())
    }

    fn event(&self, kind: EventKind, frame: Option<u64>, step: u64) -> ExecutionEvent {
        ExecutionEvent::from_context(
            &EventContext {
                arch: MachineId::from(tic80_core::MACHINE_ID),
                machine_version: VersionStamp::from(tic80_core::SEMANTICS),
                run_id: self.request.run_id.clone(),
                sequence: self.next_sequence,
                step,
                cycle_or_tick: Some(step),
                frame,
                pc: None,
                instruction: None,
            },
            kind,
        )
    }
}

fn scheduled_inputs(prepared: &PreparedRun) -> Result<Vec<ScheduledInput>, String> {
    prepared
        .input_schedule
        .entries
        .iter()
        .map(|entry| {
            let frame = match entry.coordinate {
                InputCoordinate::Frame { frame } => frame,
                _ => {
                    return Err(format!(
                        "TIC-80 input {} must use a frame coordinate",
                        entry.input_id
                    ));
                }
            };
            if entry.input_id.as_str() != "tic80.gamepad" {
                return Err(format!("unsupported TIC-80 input ID {}", entry.input_id));
            }
            let mask = match &entry.payload.value {
                StructuredValue::Unsigned(value) => u32::try_from(*value).map_err(|_| ()),
                StructuredValue::Signed(value) => u32::try_from(*value).map_err(|_| ()),
                _ => Err(()),
            }
            .map_err(|_| "TIC-80 gamepad payload must be an unsigned 32-bit value".to_string())?;
            Ok(ScheduledInput {
                ordinal: entry.ordinal,
                frame,
                input_id: entry.input_id.clone(),
                payload: entry.payload.clone(),
                mask,
            })
        })
        .collect()
}

fn sink_error(error: SinkError) -> String {
    error.to_string()
}
