//! GlassVM session orchestration over the native Wyrd-16 transition engine.

use std::collections::BTreeSet;

use glassvm_core::{
    Address, CapabilityOutput, CommonMetrics, ControlFlow, ControlFlowKind, Emission, EmissionSink,
    EmulatorSession, EventContext, EventKind, ExecutionEvent, ExecutionRequest, FrameArtifact,
    FrameCapture, InstructionRef, IoChannel, IoDirection, IoObservation, MachineId, NativeEvent,
    NativeEvidenceEnvelope, NormalizerDriver, PreparedObservation, RunResult, SnapshotArtifact,
    SnapshotCapture, StateLocation, StateRead, StateSpace, StateWrite, VersionStamp,
};
use serde_json::json;
use wyrd16_core::{
    DISPLAY_HEIGHT, DISPLAY_PIXELS, DISPLAY_WIDTH, MEMORY_BYTES, MachineState, NativeEffectKind,
    StepEffect, execute_step,
};

use crate::{
    adapters::Wyrd16NativeEventAdapter,
    emulator_backend::ScheduledKeyInput,
    identity::{
        EMULATOR_VERSION, MACHINE_ID, MACHINE_VERSION, SNAPSHOT_FORMAT, SNAPSHOT_VERSION, schema,
        snapshot_schema,
    },
    session_snapshot::{SessionLifecycle, SessionSnapshot, snapshot_digest},
    trace::{access_after, access_before, event_kind},
};

pub(crate) struct Wyrd16Session {
    state: MachineState,
    initial: MachineState,
    request: ExecutionRequest,
    coverage: BTreeSet<u16>,
    opcode_counts: [u64; 16],
    display_writes: u64,
    changed_pixels: u64,
    palette_writes: u64,
    max_frames: u64,
    max_steps: Option<u64>,
    cycles_per_frame: u32,
    scheduled_inputs: Vec<ScheduledKeyInput>,
    next_scheduled_input: usize,
    next_sequence: u64,
    lifecycle: SessionLifecycle,
    frame_active: bool,
    cycles_into_frame: u32,
    prepared_observation: PreparedObservation,
}

pub(crate) struct Wyrd16ExecutionLimits {
    pub(crate) frame: u64,
    pub(crate) step: Option<u64>,
}

impl Wyrd16Session {
    pub(crate) fn new(
        rom: &[u8],
        request: ExecutionRequest,
        prepared_observation: PreparedObservation,
        execution_limits: Wyrd16ExecutionLimits,
        cycles_per_frame: u32,
        machine_seed: u64,
        scheduled_inputs: Vec<ScheduledKeyInput>,
    ) -> Result<Self, String> {
        let initial = MachineState::boot(rom, machine_seed).map_err(|error| error.to_string())?;
        Ok(Self {
            state: initial.clone(),
            initial,
            request,
            coverage: BTreeSet::new(),
            opcode_counts: [0; 16],
            display_writes: 0,
            changed_pixels: 0,
            palette_writes: 0,
            max_frames: execution_limits.frame,
            max_steps: execution_limits.step,
            cycles_per_frame,
            scheduled_inputs,
            next_scheduled_input: 0,
            next_sequence: 0,
            lifecycle: SessionLifecycle::Fresh,
            frame_active: false,
            cycles_into_frame: 0,
            prepared_observation,
        })
    }

    fn step_once(&mut self) -> StepEffect {
        let effect = execute_step(&mut self.state);
        self.coverage.insert(effect.pc);
        self.opcode_counts[effect.rune.op as usize] += 1;
        self.display_writes += u64::from(effect.display_writes > 0);
        self.changed_pixels += effect.display_writes as u64;
        self.palette_writes += u64::from(effect.palette_write.is_some());
        effect
    }

    fn emit_effect(
        &mut self,
        effect: &StepEffect,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        let kind = event_kind(effect.kind);
        if !self
            .request
            .observation
            .normalized_events
            .events
            .includes(&kind)
        {
            return Ok(());
        }
        let context = EventContext {
            arch: MachineId::from(MACHINE_ID),
            machine_version: VersionStamp::from(MACHINE_VERSION),
            run_id: self.request.run_id.clone(),
            sequence: self.next_sequence,
            step: self.state.cycles.saturating_sub(1),
            cycle_or_tick: Some(self.state.cycles.saturating_sub(1)),
            frame: Some(self.state.frames),
            pc: Some(Address::new("arena", effect.pc as u64)),
            instruction: Some(InstructionRef {
                encoding: "wyrd16.rune16.be".into(),
                bytes: effect.rune.word.to_be_bytes().to_vec(),
                decoded: Some(effect.rune.disassemble()),
            }),
        };
        let native_kind = native_effect_kind(effect);
        let native = NativeEvent {
            schema: schema("wyrd16.event"),
            kind: native_kind.into(),
            payload: json!({
                "opcode": effect.rune.op,
                "cursor": [self.state.cursor_x, self.state.cursor_y],
                "ink": self.state.ink
            }),
        };
        let mut normalized = Wyrd16NativeEventAdapter.normalize(&context, &native)?;
        if normalized.len() != 1 {
            return Err(format!(
                "Wyrd-16 native-event adapter returned {} events for {native_kind:?}; expected one",
                normalized.len()
            ));
        }
        let mut event = normalized.remove(0);
        if event.kind != kind {
            return Err(format!(
                "Wyrd-16 native-event adapter mapped {native_kind:?} to {:?}, expected {kind:?}",
                event.kind
            ));
        }
        if let Some((register, before, after)) = effect.register_write {
            let location = StateLocation {
                space: StateSpace::Register,
                name: Some(format!("R{register}")),
                address: None,
                width_bits: Some(8),
            };
            event.writes.push(StateWrite {
                location,
                before: access_before(&self.request, before),
                after: access_after(&self.request, after),
            });
        }
        if let Some((write, address, before, after)) = effect.memory_access {
            let location =
                StateLocation::addressed(StateSpace::Memory, Address::new("arena", address as u64));
            if write {
                event.writes.push(StateWrite {
                    location,
                    before: access_before(&self.request, before),
                    after: access_after(&self.request, after),
                });
            } else {
                event.reads.push(StateRead {
                    location,
                    value: access_after(&self.request, after),
                });
            }
        }
        if effect.rune.op == 0x7 || effect.rune.op == 0x8 {
            event.control_flow = Some(ControlFlow {
                kind: ControlFlowKind::Branch,
                from: Some(Address::new("arena", effect.pc as u64)),
                to: Some(Address::new("arena", effect.next_pc as u64)),
                taken: effect.branch_taken,
            });
        }
        if effect.display_writes > 0 {
            event.io.push(IoObservation {
                port: "canvas_out".into(),
                direction: IoDirection::Output,
                channel: IoChannel::Display,
                value: json!({"changed_pixels": effect.display_writes}),
            });
        }
        if let Some((index, before, after)) = effect.palette_write {
            event.io.push(IoObservation {
                port: "palette_out".into(),
                direction: IoDirection::Output,
                channel: IoChannel::Extension("palette".into()),
                value: json!({"index": index, "before": before, "after": after}),
            });
        }
        if let Some(sample) = effect.input_sample {
            event.io.push(IoObservation {
                port: "keys_in".into(),
                direction: IoDirection::Input,
                channel: IoChannel::Keypad,
                value: json!(sample),
            });
        }
        if let Some(sample) = effect.random_sample {
            event.io.push(IoObservation {
                port: "entropy_in".into(),
                direction: IoDirection::Input,
                channel: IoChannel::Randomness,
                value: json!(sample),
            });
        }
        if !self.request.observation.native_evidence.enabled
            || !self
                .request
                .observation
                .native_evidence
                .includes(native_kind)
        {
            event.extensions.clear();
        }
        self.emit_event(event, sink)?;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| "Wyrd-16 event-sequence counter exhausted u64".to_string())?;
        Ok(())
    }

    fn emit_lifecycle(
        &mut self,
        kind: EventKind,
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
        let context = EventContext {
            arch: MachineId::from(MACHINE_ID),
            machine_version: VersionStamp::from(MACHINE_VERSION),
            run_id: self.request.run_id.clone(),
            sequence: self.next_sequence,
            step: self.state.cycles,
            cycle_or_tick: Some(self.state.cycles),
            frame: Some(self.state.frames),
            pc: Some(Address::new("arena", self.state.pc as u64)),
            instruction: None,
        };
        let event = ExecutionEvent::from_context(&context, kind);
        self.emit_event(event, sink)?;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| "Wyrd-16 event-sequence counter exhausted u64".to_string())?;
        Ok(())
    }

    fn apply_scheduled_inputs(&mut self, sink: &mut dyn EmissionSink) -> Result<(), String> {
        while let Some(scheduled_input) = self
            .scheduled_inputs
            .get(self.next_scheduled_input)
            .cloned()
        {
            let frame = scheduled_input.frame;
            if frame > self.state.frames {
                break;
            }
            if frame < self.state.frames {
                return Err(format!(
                    "Wyrd-16 stimulus {} at frame {frame} was not applied before frame {}",
                    scheduled_input.ordinal, self.state.frames
                ));
            }
            let before = self.state.input_mask;
            let mask = if scheduled_input.active {
                before | (1_u8 << scheduled_input.key)
            } else {
                before & !(1_u8 << scheduled_input.key)
            };
            self.state.input_mask = mask;

            let kind = EventKind::InputApplied;
            if self
                .request
                .observation
                .normalized_events
                .events
                .includes(&kind)
            {
                let context = EventContext {
                    arch: MachineId::from(MACHINE_ID),
                    machine_version: VersionStamp::from(MACHINE_VERSION),
                    run_id: self.request.run_id.clone(),
                    sequence: self.next_sequence,
                    step: self.state.cycles,
                    cycle_or_tick: Some(self.state.cycles),
                    frame: Some(self.state.frames),
                    pc: Some(Address::new("arena", self.state.pc as u64)),
                    instruction: None,
                };
                let mut event = ExecutionEvent::from_context(&context, kind);
                event.writes.push(StateWrite {
                    location: StateLocation {
                        space: StateSpace::Input,
                        name: Some("keys".into()),
                        address: None,
                        width_bits: Some(8),
                    },
                    before: access_before(&self.request, before),
                    after: access_after(&self.request, mask),
                });
                event.io.push(IoObservation {
                    port: "keys_in".into(),
                    direction: IoDirection::Input,
                    channel: IoChannel::Keypad,
                    value: json!({"key": scheduled_input.key, "active": scheduled_input.active, "ordinal": scheduled_input.ordinal}),
                });
                self.emit_event(event, sink)?;
                self.next_sequence = self
                    .next_sequence
                    .checked_add(1)
                    .ok_or_else(|| "Wyrd-16 event-sequence counter exhausted u64".to_string())?;
            }
            self.next_scheduled_input += 1;
        }
        Ok(())
    }

    fn run_frame(&mut self, sink: &mut dyn EmissionSink) -> Result<(), String> {
        if self.step_limit_reached() {
            return Ok(());
        }
        if !self.frame_active {
            if self.state.halted || self.state.frames >= self.max_frames {
                return Ok(());
            }
            self.apply_scheduled_inputs(sink)?;
            self.frame_active = true;
            self.cycles_into_frame = 0;
        }

        while self.cycles_into_frame < self.cycles_per_frame {
            if self.state.halted || self.step_limit_reached() {
                break;
            }
            let effect = self.step_once();
            self.cycles_into_frame += 1;
            self.emit_effect(&effect, sink)?;
            if let SnapshotCapture::EverySteps(interval) =
                self.request.observation.snapshots.capture
                && self.state.cycles.is_multiple_of(interval)
            {
                let snapshot = SnapshotArtifact::new_typed(
                    self.request.run_id.clone(),
                    MachineId::from(MACHINE_ID),
                    VersionStamp::from(MACHINE_VERSION),
                    snapshot_schema(),
                    self.next_sequence,
                    self.state.cycles,
                    self.snapshot_bytes_for(SessionLifecycle::Incremental)?,
                );
                sink.emit(Emission::Snapshot(&snapshot))
                    .map_err(|error| error.to_string())?;
            }
        }
        if self.step_limit_reached()
            && !self.state.halted
            && self.cycles_into_frame < self.cycles_per_frame
        {
            return Ok(());
        }

        self.state.frames = self
            .state
            .frames
            .checked_add(1)
            .ok_or_else(|| "Wyrd-16 frame counter exhausted u64".to_string())?;
        let frame = self.state.frames.saturating_sub(1);
        let frame_sequence = self.next_sequence;
        if self
            .request
            .observation
            .normalized_events
            .events
            .includes(&EventKind::FrameCompleted)
        {
            let context = EventContext {
                arch: MachineId::from(MACHINE_ID),
                machine_version: VersionStamp::from(MACHINE_VERSION),
                run_id: self.request.run_id.clone(),
                sequence: frame_sequence,
                step: self.state.cycles,
                cycle_or_tick: Some(self.state.cycles),
                frame: Some(frame),
                pc: Some(Address::new("arena", self.state.pc as u64)),
                instruction: None,
            };
            self.emit_event(
                ExecutionEvent::from_context(&context, EventKind::FrameCompleted),
                sink,
            )?;
            self.next_sequence = self.next_sequence.checked_add(1).ok_or_else(|| {
                "Wyrd-16 event-sequence counter exhausted at frame completion".to_string()
            })?;
        }
        let artifact = match self.request.observation.frames.capture {
            FrameCapture::None => None,
            FrameCapture::Hashes => Some(FrameArtifact::fingerprint(
                self.request.run_id.clone(),
                MachineId::from(MACHINE_ID),
                VersionStamp::from(MACHINE_VERSION),
                schema("wyrd16.canvas_fingerprint"),
                frame_sequence,
                self.state.cycles,
                frame,
                self.hash_canvas().to_le_bytes().to_vec(),
            )),
            FrameCapture::Full => Some(FrameArtifact::full(
                self.request.run_id.clone(),
                MachineId::from(MACHINE_ID),
                VersionStamp::from(MACHINE_VERSION),
                schema("wyrd16.canvas_indexed4"),
                frame_sequence,
                self.state.cycles,
                frame,
                self.state.canvas.clone(),
            )),
        };
        if let Some(artifact) = artifact {
            sink.emit(Emission::Frame(&artifact))
                .map_err(|error| error.to_string())?;
        }
        self.frame_active = false;
        self.cycles_into_frame = 0;
        Ok(())
    }

    fn hash_canvas(&self) -> u64 {
        self.state
            .canvas
            .iter()
            .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
            })
    }

    fn step_limit_reached(&self) -> bool {
        self.max_steps
            .is_some_and(|limit| self.state.cycles >= limit)
    }

    fn emit_event(
        &mut self,
        event: ExecutionEvent,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        sink.emit(Emission::Event(&event))
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn observations(&self) -> Vec<NativeEvidenceEnvelope> {
        vec![
            NativeEvidenceEnvelope::new(
                self.request.run_id.clone(),
                MachineId::from(MACHINE_ID),
                VersionStamp::from(MACHINE_VERSION),
                self.next_sequence,
                self.state.cycles,
                NativeEvent {
                    schema: schema("wyrd16.observation"),
                    kind: "wyrd16.canvas".into(),
                    payload: json!({
                        "width": 64,
                        "height": 64,
                        "planes": 4,
                        "bytes": self.state.canvas
                    }),
                },
            ),
            NativeEvidenceEnvelope::new(
                self.request.run_id.clone(),
                MachineId::from(MACHINE_ID),
                VersionStamp::from(MACHINE_VERSION),
                self.next_sequence.saturating_add(1),
                self.state.cycles,
                NativeEvent {
                    schema: schema("wyrd16.observation"),
                    kind: "wyrd16.palette".into(),
                    payload: json!({"format": "rgb332", "entries": self.state.palette}),
                },
            ),
        ]
    }

    fn result(
        &self,
        termination: &str,
        normalized: &[CapabilityOutput],
    ) -> Result<RunResult, String> {
        Ok(RunResult {
            common: CommonMetrics {
                cycles: self.state.cycles,
                frames: self.state.frames,
                termination: termination.into(),
                boot_success: true,
            },
            capabilities: normalized.to_vec(),
        })
    }

    fn snapshot_payload_for(&self, lifecycle: SessionLifecycle) -> Result<SessionSnapshot, String> {
        let mut snapshot = SessionSnapshot {
            format: SNAPSHOT_FORMAT.into(),
            version: SNAPSHOT_VERSION,
            machine_version: MACHINE_VERSION.into(),
            emulator_version: EMULATOR_VERSION.into(),
            request: self.request.clone(),
            initial: self.initial.clone(),
            state: self.state.clone(),
            coverage: self.coverage.clone(),
            opcode_counts: self.opcode_counts,
            display_writes: self.display_writes,
            changed_pixels: self.changed_pixels,
            palette_writes: self.palette_writes,
            next_scheduled_input: self.next_scheduled_input,
            next_sequence: self.next_sequence,
            lifecycle,
            frame_active: self.frame_active,
            cycles_into_frame: self.cycles_into_frame,
            payload_digest: glassvm_core::ContentDigest::sha256(&[]),
        };
        snapshot.payload_digest = snapshot_digest(&snapshot)?;
        Ok(snapshot)
    }

    fn snapshot_payload(&self) -> Result<SessionSnapshot, String> {
        self.snapshot_payload_for(self.lifecycle)
    }

    fn snapshot_bytes_for(&self, lifecycle: SessionLifecycle) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&self.snapshot_payload_for(lifecycle)?)
            .map_err(|error| error.to_string())
    }

    fn snapshot_bytes(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&self.snapshot_payload()?).map_err(|error| error.to_string())
    }

    fn validate_snapshot(&self, snapshot: &SessionSnapshot) -> Result<(), String> {
        if snapshot.payload_digest != snapshot_digest(snapshot)? {
            return Err("Wyrd-16 snapshot payload digest mismatch".into());
        }
        if snapshot.format != SNAPSHOT_FORMAT
            || snapshot.version != SNAPSHOT_VERSION
            || snapshot.machine_version != MACHINE_VERSION
            || snapshot.emulator_version != EMULATOR_VERSION
        {
            return Err("snapshot uses an unsupported Wyrd-16 format or version".into());
        }
        if snapshot.request != self.request {
            return Err("snapshot execution request does not match this Wyrd-16 session".into());
        }
        if snapshot.initial != self.initial {
            return Err("snapshot initial state does not match this Wyrd-16 ROM and seed".into());
        }
        if snapshot.state.memory.len() != MEMORY_BYTES
            || snapshot.state.canvas.len() != DISPLAY_PIXELS
        {
            return Err("snapshot has invalid Wyrd-16 memory or canvas dimensions".into());
        }
        if snapshot.state.pc as usize >= MEMORY_BYTES || !snapshot.state.pc.is_multiple_of(2) {
            return Err("snapshot has an invalid Wyrd-16 program counter".into());
        }
        if snapshot.state.cursor_x >= DISPLAY_WIDTH as u8
            || snapshot.state.cursor_y >= DISPLAY_HEIGHT as u8
            || snapshot.state.ink > 0x0f
            || snapshot.state.canvas.iter().any(|pixel| *pixel > 0x0f)
        {
            return Err("snapshot has invalid Wyrd-16 canvas state".into());
        }
        if snapshot.state.frames > self.max_frames
            || self
                .max_steps
                .is_some_and(|limit| snapshot.state.cycles > limit)
            || snapshot.cycles_into_frame > self.cycles_per_frame
            || snapshot.frame_active && snapshot.state.frames >= self.max_frames
            || !snapshot.frame_active && snapshot.cycles_into_frame != 0
        {
            return Err("snapshot has an impossible Wyrd-16 frame phase".into());
        }
        if snapshot.frame_active && snapshot.lifecycle == SessionLifecycle::Fresh {
            return Err("snapshot cannot have an active Wyrd-16 frame before startup".into());
        }

        let executed_opcodes = snapshot
            .opcode_counts
            .iter()
            .try_fold(0_u64, |total, count| total.checked_add(*count))
            .ok_or_else(|| "snapshot Wyrd-16 opcode counters overflow u64".to_string())?;
        if executed_opcodes != snapshot.state.cycles
            || snapshot.display_writes > snapshot.state.cycles
            || snapshot.palette_writes > snapshot.state.cycles
            || snapshot
                .coverage
                .iter()
                .any(|address| *address as usize >= MEMORY_BYTES || !address.is_multiple_of(2))
        {
            return Err("snapshot has inconsistent Wyrd-16 execution counters".into());
        }

        let expected_scheduled_input = self
            .scheduled_inputs
            .iter()
            .take_while(|scheduled_input| {
                let frame = scheduled_input.frame;
                frame < snapshot.state.frames
                    || snapshot.frame_active && frame == snapshot.state.frames
            })
            .count();
        if snapshot.next_scheduled_input != expected_scheduled_input {
            return Err(format!(
                "snapshot has Wyrd-16 scheduled-input cursor {}; expected {expected_scheduled_input}",
                snapshot.next_scheduled_input
            ));
        }

        if snapshot.lifecycle == SessionLifecycle::Fresh
            && (snapshot.state != snapshot.initial
                || !snapshot.coverage.is_empty()
                || executed_opcodes != 0
                || snapshot.display_writes != 0
                || snapshot.changed_pixels != 0
                || snapshot.palette_writes != 0
                || snapshot.next_scheduled_input != 0
                || snapshot.next_sequence != 0
                || snapshot.frame_active
                || snapshot.cycles_into_frame != 0)
        {
            return Err("snapshot labels an advanced Wyrd-16 session as unstarted".into());
        }
        Ok(())
    }

    fn reject_snapshot_transition(&self, snapshot: &SessionSnapshot) -> Result<(), String> {
        if self.lifecycle == SessionLifecycle::Terminal {
            return Err("Wyrd-16 session is terminal; reset before restoring".into());
        }
        if self.lifecycle == SessionLifecycle::Incremental
            && (snapshot.lifecycle == SessionLifecycle::Fresh
                || snapshot.state.cycles <= self.state.cycles)
        {
            return Err("Wyrd-16 snapshot would rewind or repeat current progress".into());
        }
        Ok(())
    }
}

impl EmulatorSession for Wyrd16Session {
    fn request(&self) -> &ExecutionRequest {
        &self.request
    }

    fn execute(&mut self, sink: &mut dyn EmissionSink) -> Result<RunResult, String> {
        if self.lifecycle != SessionLifecycle::Fresh
            || self.state != self.initial
            || !self.coverage.is_empty()
            || self.opcode_counts != [0; 16]
            || self.display_writes != 0
            || self.changed_pixels != 0
            || self.palette_writes != 0
            || self.next_scheduled_input != 0
            || self.next_sequence != 0
            || self.frame_active
            || self.cycles_into_frame != 0
        {
            return Err(
                "Wyrd-16 execute must begin at canonical boot; reset after stepping, restoring, or changing input"
                    .into(),
            );
        }
        let mut normalizer_driver = NormalizerDriver::new_with_prepared_observation(
            crate::normalizer::Wyrd16Normalizer::new(),
            sink,
            &self.prepared_observation,
        );
        let sink: &mut dyn EmissionSink = &mut normalizer_driver;
        self.lifecycle = SessionLifecycle::Terminal;
        sink.emit(Emission::RunStarted(&self.request))
            .map_err(|error| error.to_string())?;
        self.emit_lifecycle(EventKind::RunStarted, sink)?;

        while self.state.frames < self.max_frames
            && !self.state.halted
            && !self.step_limit_reached()
        {
            self.run_frame(sink)?;
        }

        let final_state = if matches!(
            self.request.observation.snapshots.capture,
            SnapshotCapture::Final | SnapshotCapture::EverySteps(_)
        ) {
            let snapshot = SnapshotArtifact::new_typed(
                self.request.run_id.clone(),
                MachineId::from(MACHINE_ID),
                VersionStamp::from(MACHINE_VERSION),
                snapshot_schema(),
                self.next_sequence,
                self.state.cycles,
                self.snapshot_bytes()?,
            );
            sink.emit(Emission::Snapshot(&snapshot))
                .map_err(|error| error.to_string())?;
            Some(snapshot)
        } else {
            None
        };
        self.emit_lifecycle(EventKind::RunHalted, sink)?;
        let termination = if self.state.halted {
            "halted"
        } else if self.step_limit_reached() {
            "step_limit"
        } else {
            "frame_budget"
        };
        if !self
            .prepared_observation
            .native_observation_schemas
            .is_empty()
        {
            for observation in self.observations() {
                sink.emit(Emission::NativeEvidence(&observation))
                    .map_err(|error| error.to_string())?;
            }
        }
        let normalizer_run = normalizer_driver
            .finish(final_state.as_ref())
            .map_err(|error| error.to_string())?;
        let (mut downstream, normalizer_output) = normalizer_run.into_parts();
        let normalized = normalized_capabilities(&normalizer_output);
        let result = self.result(termination, &normalized)?;
        let sink: &mut dyn EmissionSink = &mut downstream;
        sink.emit(Emission::RunFinished(&result))
            .map_err(|error| error.to_string())?;
        Ok(result)
    }

    fn step_frame(&mut self) -> Result<(), String> {
        if self.lifecycle == SessionLifecycle::Terminal {
            return Err("Wyrd-16 session is terminal; reset before stepping".into());
        }
        if self.state.halted && !self.frame_active || self.state.frames >= self.max_frames {
            return Ok(());
        }
        self.lifecycle = SessionLifecycle::Incremental;
        let mut sink = glassvm_core::NullSink;
        let result = self.run_frame(&mut sink);
        if result.is_ok() && self.step_limit_reached() {
            self.lifecycle = SessionLifecycle::Terminal;
        }
        result
    }

    fn reset(&mut self) -> Result<(), String> {
        self.state = self.initial.clone();
        self.coverage.clear();
        self.opcode_counts = [0; 16];
        self.display_writes = 0;
        self.changed_pixels = 0;
        self.palette_writes = 0;
        self.next_scheduled_input = 0;
        self.next_sequence = 0;
        self.lifecycle = SessionLifecycle::Fresh;
        self.frame_active = false;
        self.cycles_into_frame = 0;
        Ok(())
    }

    fn snapshot(&self) -> Result<Vec<u8>, String> {
        self.snapshot_bytes()
    }

    fn restore_snapshot(&mut self, bytes: &[u8]) -> Result<(), String> {
        let preflight: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid Wyrd-16 snapshot: {error}"))?;
        let version = preflight.get("version").and_then(serde_json::Value::as_u64);
        if version != Some(u64::from(SNAPSHOT_VERSION)) {
            return Err(format!(
                "unsupported Wyrd-16 snapshot version {:?}; restart the run with snapshot format v{}",
                version, SNAPSHOT_VERSION
            ));
        }
        let restored: SessionSnapshot = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid Wyrd-16 snapshot: {error}"))?;
        self.validate_snapshot(&restored)?;
        self.reject_snapshot_transition(&restored)?;
        self.state = restored.state;
        self.coverage = restored.coverage;
        self.opcode_counts = restored.opcode_counts;
        self.display_writes = restored.display_writes;
        self.changed_pixels = restored.changed_pixels;
        self.palette_writes = restored.palette_writes;
        self.next_scheduled_input = restored.next_scheduled_input;
        self.next_sequence = restored.next_sequence;
        self.lifecycle = restored.lifecycle;
        self.frame_active = restored.frame_active;
        self.cycles_into_frame = restored.cycles_into_frame;
        Ok(())
    }
}

fn native_effect_kind(effect: &StepEffect) -> &'static str {
    match effect.kind {
        NativeEffectKind::InstructionDecoded => "instruction",
        NativeEffectKind::RunHalted => "halt",
        NativeEffectKind::RegisterWrite => {
            if effect.rune.op == 0x9 {
                "random"
            } else {
                "register"
            }
        }
        NativeEffectKind::MemoryRead => "load",
        NativeEffectKind::MemoryWrite => "store",
        NativeEffectKind::BranchTaken => {
            if effect.rune.op == 0x7 {
                "jump"
            } else {
                "branch"
            }
        }
        NativeEffectKind::DisplayWrite => match effect.rune.op {
            0x0 => "clear",
            0xC => "pixel",
            0xD => "line",
            0xE => "palette",
            0xF => "weave",
            _ => "pixel",
        },
        NativeEffectKind::InputSampled => "key",
    }
}

fn normalized_capabilities(output: &glassvm_core::NormalizerOutput) -> Vec<CapabilityOutput> {
    output.capabilities.clone()
}
