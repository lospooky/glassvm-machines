use glassvm_core::{
    CommonMetrics, ContentDigest, Emission, EmissionSink, EmulatorSession, EventContext, EventKind,
    ExecutionEvent, ExecutionRequest, FrameArtifact, FrameCapture, FrameEvidence, InputApplied,
    InputCoordinate, InputId, InputSource, InputValueEvidence, MachineId, NativeEvent,
    NativeEvidenceEnvelope, PreparedObservation, PreparedRun, RunResult, SinkError,
    SnapshotArtifact, SnapshotCapture, StructuredValue, TypedInputPayload, VersionStamp,
    canonical_json_fingerprint,
};
use pico8_core::{Cartridge, Pico8Runtime, RuntimeSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::emulator_backend::configuration_values;
use crate::identity::{MACHINE_ID, SEMANTICS, schema};

#[derive(Clone)]
struct ScheduledInput {
    ordinal: u64,
    frame: u64,
    input_id: InputId,
    payload: TypedInputPayload,
    bit: u16,
    active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pico8MachineState {
    ram: Vec<u8>,
    frame: u64,
    input_mask: u16,
    previous_input_mask: u16,
    rng_state: u64,
    callback_hz: u32,
    draw_color: u8,
    draw_palette: [u8; 16],
    display_palette: [u8; 16],
    transparent: [bool; 16],
    camera_x: i32,
    camera_y: i32,
    clip: [i32; 4],
}

impl Pico8MachineState {
    fn from_native(native: &RuntimeSnapshot) -> Self {
        Self {
            ram: native.ram.clone(),
            frame: native.frame,
            input_mask: native.input_mask,
            previous_input_mask: native.previous_input_mask,
            rng_state: native.rng_state,
            callback_hz: native.callback_hz,
            draw_color: native.draw_color,
            draw_palette: native.draw_palette,
            display_palette: native.display_palette,
            transparent: native.transparent,
            camera_x: native.camera_x,
            camera_y: native.camera_y,
            clip: native.clip,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pico8SessionSnapshot {
    format: String,
    version: u32,
    request_digest: ContentDigest,
    machine_state: Pico8MachineState,
    next_input: usize,
}

pub(super) struct Pico8Session {
    request: ExecutionRequest,
    artifact: Vec<u8>,
    runtime: Pico8Runtime,
    scheduled_inputs: Vec<ScheduledInput>,
    next_input: usize,
    cycles_per_frame: u32,
    step_limit: Option<u64>,
    instruction_budget: u64,
    machine_seed: u64,
    prepared_observation: PreparedObservation,
    next_sequence: u64,
    terminal: bool,
}

impl Pico8Session {
    pub(super) fn new(
        artifact: &[u8],
        request: ExecutionRequest,
        prepared_run: PreparedRun,
        prepared_observation: PreparedObservation,
    ) -> Result<Self, String> {
        let (cycles_per_frame, instruction_budget, machine_seed) =
            configuration_values(&prepared_run.configuration)?;
        let cartridge = Cartridge::parse(artifact)
            .map_err(|error| format!("invalid PICO-8 cartridge: {error}"))?;
        let runtime = Pico8Runtime::new(&cartridge, machine_seed, instruction_budget)?;
        let scheduled_inputs = scheduled_inputs(&prepared_run)?;
        let step_limit = prepared_run.execution_controls.step_limit;
        Ok(Self {
            request,
            artifact: artifact.to_vec(),
            runtime,
            scheduled_inputs,
            next_input: 0,
            cycles_per_frame,
            step_limit,
            instruction_budget,
            machine_seed,
            prepared_observation,
            next_sequence: 0,
            terminal: false,
        })
    }

    fn apply_inputs(&mut self, frame: u64, sink: &mut dyn EmissionSink) -> Result<(), String> {
        while let Some(input) = self.scheduled_inputs.get(self.next_input).cloned() {
            if input.frame > frame {
                break;
            }
            if input.frame < frame {
                return Err(format!(
                    "PICO-8 scheduled input for frame {} was not applied before frame {frame}",
                    input.frame
                ));
            }
            let mut mask = self.runtime.snapshot().input_mask;
            if input.active {
                mask |= input.bit;
            } else {
                mask &= !input.bit;
            }
            self.runtime.set_input_mask(mask);
            self.emit_input_applied(&input, frame, mask, sink)?;
            self.next_input += 1;
        }
        Ok(())
    }

    fn reconstruct_runtime(
        &self,
        snapshot: &Pico8SessionSnapshot,
    ) -> Result<(Pico8Runtime, usize), String> {
        let cartridge = Cartridge::parse(&self.artifact)
            .map_err(|error| format!("invalid PICO-8 cartridge: {error}"))?;
        let runtime = Pico8Runtime::new(&cartridge, self.machine_seed, self.instruction_budget)?;
        let mut next_input = 0;

        // Lua globals, tables, and closures are part of execution state but
        // cannot be copied out of mlua. Recreate them by replaying the
        // deterministic cartridge callbacks with the prepared schedule.
        for frame in 0..snapshot.machine_state.frame {
            while let Some(input) = self.scheduled_inputs.get(next_input) {
                if input.frame < frame {
                    return Err(format!(
                        "PICO-8 scheduled input for frame {} was not replayed before frame {frame}",
                        input.frame
                    ));
                }
                if input.frame > frame {
                    break;
                }
                let mut mask = runtime.snapshot().input_mask;
                if input.active {
                    mask |= input.bit;
                } else {
                    mask &= !input.bit;
                }
                runtime.set_input_mask(mask);
                next_input += 1;
            }
            runtime.step_frame()?;
        }

        if next_input != snapshot.next_input {
            return Err(format!(
                "PICO-8 snapshot input cursor {} does not match reconstructed cursor {next_input}",
                snapshot.next_input
            ));
        }
        if Pico8MachineState::from_native(&runtime.snapshot()) != snapshot.machine_state {
            return Err(
                "PICO-8 snapshot machine state does not match deterministic reconstruction".into(),
            );
        }
        Ok((runtime, next_input))
    }

    fn step_native_frame(&mut self) -> Result<(), String> {
        let frame = self.runtime.frame();
        self.apply_inputs(frame, &mut glassvm_core::NullSink)?;
        self.runtime.step_frame()
    }

    fn frame_limit(&self) -> Result<u64, String> {
        self.request
            .execution_controls
            .frame_limit
            .ok_or_else(|| "PICO-8 execution requires an explicit frame_limit".into())
    }

    fn step_limit_reached(&self) -> bool {
        self.step_limit
            .is_some_and(|limit| self.runtime.frame() >= limit)
    }

    fn frame_limit_reached(&self) -> Result<bool, String> {
        Ok(self.runtime.frame() >= self.frame_limit()?)
    }

    fn reset_runtime(&mut self) -> Result<(), String> {
        let cartridge = Cartridge::parse(&self.artifact)
            .map_err(|error| format!("invalid PICO-8 cartridge: {error}"))?;
        self.runtime = Pico8Runtime::new(&cartridge, self.machine_seed, self.instruction_budget)?;
        self.next_input = 0;
        self.next_sequence = 0;
        self.terminal = false;
        Ok(())
    }

    fn request_digest(&self) -> Result<ContentDigest, String> {
        canonical_json_fingerprint("pico8.session_request", &self.request)
    }

    fn snapshot_bytes(&self) -> Result<Vec<u8>, String> {
        let snapshot = Pico8SessionSnapshot {
            format: crate::identity::SESSION_SNAPSHOT_FORMAT.into(),
            version: crate::identity::SESSION_SNAPSHOT_FORMAT_VERSION,
            request_digest: self.request_digest()?,
            machine_state: Pico8MachineState::from_native(&self.runtime.snapshot()),
            next_input: self.next_input,
        };
        serde_json::to_vec(&snapshot)
            .map_err(|error| format!("PICO-8 session snapshot serialization failed: {error}"))
    }

    fn emit_snapshot(&mut self, sink: &mut dyn EmissionSink) -> Result<SnapshotArtifact, String> {
        let step = self.runtime.frame();
        let snapshot = SnapshotArtifact::new_typed(
            self.request.run_id.clone(),
            MachineId::from(MACHINE_ID),
            VersionStamp::from(SEMANTICS),
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
            .ok_or_else(|| "PICO-8 emission sequence counter exhausted u64".to_string())?;
        Ok(snapshot)
    }
}

impl EmulatorSession for Pico8Session {
    fn request(&self) -> &ExecutionRequest {
        &self.request
    }

    fn execute(&mut self, sink: &mut dyn EmissionSink) -> Result<RunResult, String> {
        if self.terminal {
            return Err("PICO-8 session is terminal; reset before execute".into());
        }
        self.prepared_observation.validate()?;
        let frame_limit = self.frame_limit()?;
        let normalizer = crate::normalizer::Pico8Normalizer::new();
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
        let mut runtime_error = None;
        let mut final_state = None;
        while self.runtime.frame() < frame_limit && !self.step_limit_reached() {
            let frame = self.runtime.frame();
            if let Err(error) = self
                .apply_inputs(frame, sink)
                .and_then(|_| self.runtime.step_frame())
            {
                runtime_error = Some(error);
                break;
            }
            let completed_frame = self.runtime.frame().saturating_sub(1);
            let snapshot = self.runtime.snapshot();
            self.emit_native(
                "pico8.frame_completed",
                json!({
                    "frame": completed_frame,
                    "draw_calls": snapshot.draw_calls,
                    "audio_calls": snapshot.audio_calls,
                }),
                completed_frame,
                sink,
            )?;
            self.emit_normalized(
                EventKind::FrameCompleted,
                Some(completed_frame),
                completed_frame,
                sink,
            )?;
            self.emit_frame_artifact(completed_frame, sink)?;
            if let SnapshotCapture::EverySteps(interval) =
                self.request.observation.snapshots.capture
                && interval != 0
                && self.runtime.frame().is_multiple_of(interval)
            {
                final_state = Some(self.emit_snapshot(sink)?);
            }
        }
        self.terminal = true;
        let frames = self.runtime.frame();
        let termination = if runtime_error.is_some() {
            "runtime_error"
        } else if self.runtime.frame() >= frame_limit {
            "frame_limit"
        } else if self.step_limit_reached() {
            "step_limit"
        } else {
            "machine_exit"
        };
        if let Some(error) = &runtime_error {
            self.emit_native(
                "pico8.runtime_error",
                json!({"message": error}),
                frames,
                sink,
            )?;
            self.emit_normalized(EventKind::RunCrashed, Some(frames), frames, sink)?;
        } else {
            self.emit_normalized(EventKind::RunHalted, Some(frames), frames, sink)?;
        }
        if matches!(
            self.request.observation.snapshots.capture,
            SnapshotCapture::Final
        ) {
            final_state = Some(self.emit_snapshot(sink)?);
        }
        let snapshot = self.runtime.snapshot();
        self.emit_native(
            "pico8.execution_summary",
            json!({
                "frames": snapshot.frame,
                "draw_calls": snapshot.draw_calls,
                "audio_calls": snapshot.audio_calls,
                "print_calls": snapshot.printed.len(),
                "callback_hz": self.runtime.fps(),
                "termination": termination,
                "compatibility_tier": "documented-headless-subset",
                "numeric_model": "host-f64",
            }),
            frames,
            sink,
        )?;
        let mut result = RunResult {
            common: CommonMetrics {
                cycles: frames.saturating_mul(u64::from(self.cycles_per_frame)),
                frames,
                termination: termination.into(),
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
        if let Some(error) = runtime_error {
            return Err(error);
        }
        Ok(result)
    }

    fn step_frame(&mut self) -> Result<(), String> {
        if self.terminal {
            return Err("PICO-8 session is terminal; reset before stepping".into());
        }
        if self.frame_limit_reached()? || self.step_limit_reached() {
            self.terminal = true;
            return Err("PICO-8 session reached an execution limit".into());
        }
        self.step_native_frame()
    }

    fn reset(&mut self) -> Result<(), String> {
        self.reset_runtime()
    }

    fn snapshot(&self) -> Result<Vec<u8>, String> {
        self.snapshot_bytes()
    }

    fn restore_snapshot(&mut self, bytes: &[u8]) -> Result<(), String> {
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid PICO-8 session snapshot: {error}"))?;
        let version = value.get("version").and_then(serde_json::Value::as_u64);
        if version != Some(u64::from(crate::identity::SESSION_SNAPSHOT_FORMAT_VERSION)) {
            return Err(format!(
                "unsupported PICO-8 session snapshot version {:?}; restart the run with snapshot format v{}",
                version,
                crate::identity::SESSION_SNAPSHOT_FORMAT_VERSION
            ));
        }
        let snapshot: Pico8SessionSnapshot = serde_json::from_value(value)
            .map_err(|error| format!("invalid PICO-8 session snapshot: {error}"))?;
        if snapshot.format != crate::identity::SESSION_SNAPSHOT_FORMAT {
            return Err(format!(
                "unsupported PICO-8 session snapshot format {:?}",
                snapshot.format
            ));
        }
        if snapshot.request_digest != self.request_digest()? {
            return Err("PICO-8 session snapshot execution identity mismatch".into());
        }
        if snapshot.next_input > self.scheduled_inputs.len() {
            return Err("PICO-8 session snapshot input cursor exceeds its schedule".into());
        }
        if snapshot.machine_state.frame > self.frame_limit()?
            || self
                .step_limit
                .is_some_and(|limit| snapshot.machine_state.frame > limit)
        {
            return Err("PICO-8 session snapshot exceeds an execution limit".into());
        }
        let (runtime, next_input) = self.reconstruct_runtime(&snapshot)?;
        self.runtime = runtime;
        self.next_input = next_input;
        self.next_sequence = 0;
        self.terminal = false;
        Ok(())
    }
}

impl Pico8Session {
    fn emit_input_applied(
        &mut self,
        input: &ScheduledInput,
        frame: u64,
        mask: u16,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        let source = InputSource::Scheduled {
            ordinal: input.ordinal,
        };
        let application_coordinate = InputCoordinate::frame(frame);
        let mut event = self.event(EventKind::InputApplied, Some(frame), frame);
        event.extensions.insert(
            "glassvm.input_applied".into(),
            serde_json::to_value(InputApplied {
                input_id: input.input_id.clone(),
                input_schema: input.payload.schema.clone(),
                source,
                application_coordinate: application_coordinate.clone(),
            })
            .map_err(|error| format!("encode PICO-8 InputApplied: {error}"))?,
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
            "pico8.input_applied",
            json!({
                "input_id": input.input_id,
                "ordinal": input.ordinal,
                "mask": mask,
            }),
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
                source,
                application_coordinate,
                input.payload.clone(),
            );
            sink.emit(Emission::InputValueEvidence(&evidence))
                .map_err(sink_error)?;
        }
        Ok(())
    }

    fn emit_frame_artifact(
        &mut self,
        frame: u64,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        let pixels = self.runtime.framebuffer();
        let evidence = match self.request.observation.frames.capture {
            FrameCapture::None => return Ok(()),
            FrameCapture::Hashes => FrameEvidence::Fingerprint {
                bytes: ContentDigest::sha256(&pixels).value.to_vec(),
            },
            FrameCapture::Full => FrameEvidence::Full { bytes: pixels },
        };
        let artifact = match evidence {
            FrameEvidence::Fingerprint { bytes } => FrameArtifact::fingerprint(
                self.request.run_id.clone(),
                MachineId::from(MACHINE_ID),
                VersionStamp::from(SEMANTICS),
                schema("pico8.frame.palette_indices.sha256"),
                self.next_sequence,
                frame,
                frame,
                bytes,
            ),
            FrameEvidence::Full { bytes } => FrameArtifact::full(
                self.request.run_id.clone(),
                MachineId::from(MACHINE_ID),
                VersionStamp::from(SEMANTICS),
                schema("pico8.frame.palette_indices"),
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
            .ok_or_else(|| "PICO-8 emission sequence counter exhausted u64".to_string())?;
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
            .ok_or_else(|| "PICO-8 emission sequence counter exhausted u64".to_string())?;
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
            MachineId::from(MACHINE_ID),
            VersionStamp::from(SEMANTICS),
            self.next_sequence,
            step,
            NativeEvent {
                schema: schema("pico8.native_event"),
                kind: kind.into(),
                payload,
            },
        );
        sink.emit(Emission::NativeEvidence(&evidence))
            .map_err(sink_error)?;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| "PICO-8 emission sequence counter exhausted u64".to_string())?;
        Ok(())
    }

    fn event(&self, kind: EventKind, frame: Option<u64>, step: u64) -> ExecutionEvent {
        ExecutionEvent::from_context(
            &EventContext {
                arch: MachineId::from(MACHINE_ID),
                machine_version: VersionStamp::from(SEMANTICS),
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
                        "PICO-8 input {} must use a frame coordinate",
                        entry.input_id
                    ));
                }
            };
            let button = entry
                .input_id
                .as_str()
                .rsplit('.')
                .next()
                .ok_or_else(|| format!("invalid PICO-8 input ID {}", entry.input_id))?
                .chars()
                .next()
                .and_then(|digit| digit.to_digit(16))
                .ok_or_else(|| format!("invalid PICO-8 button input ID {}", entry.input_id))?;
            let active = match &entry.payload.value {
                StructuredValue::Bool(value) => *value,
                StructuredValue::Unsigned(value) => match value {
                    0 => false,
                    1 => true,
                    _ => return Err("PICO-8 button payload must be boolean or 0/1".into()),
                },
                StructuredValue::Signed(value) => match value {
                    0 => false,
                    1 => true,
                    _ => return Err("PICO-8 button payload must be boolean or 0/1".into()),
                },
                _ => return Err("PICO-8 button payload must be boolean or 0/1".into()),
            };
            Ok(ScheduledInput {
                ordinal: entry.ordinal,
                frame,
                input_id: entry.input_id.clone(),
                payload: entry.payload.clone(),
                bit: 1_u16 << button,
                active,
            })
        })
        .collect()
}

fn sink_error(error: SinkError) -> String {
    error.to_string()
}
