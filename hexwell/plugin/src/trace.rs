//! Hexwell: Typed event, trace-chunk, frame, and snapshot emission.

use glassvm_core::{
    AccessDetail, Address, Emission, EmissionSink, EventContext, EventKind, ExecutionEvent,
    ExecutionRequest, FrameArtifact, FrameCapture, InstructionRef, IoChannel, IoDirection,
    IoObservation, MachineId, NativeEvent, SnapshotArtifact, StateLocation, StateSpace, StateWrite,
    VersionStamp,
};
use serde_json::{Value, json};

use crate::adapters::HexwellNativeEventAdapter;
use crate::emulator_session::HexwellSession;
use crate::identity::{MACHINE_ID, MACHINE_VERSION, schema, session_snapshot_schema};
use crate::session_snapshot::SessionLifecycle;
use hexwell_core::{Catalyst, CellChange, GRID_HEIGHT, GRID_WIDTH, SweepOutcome};

impl HexwellSession {
    pub(crate) fn emit_lifecycle(
        &mut self,
        kind: EventKind,
        sequence: &mut u64,
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
        let context = self.context(*sequence, None, None);
        let event = ExecutionEvent::from_context(&context, kind);
        self.emit_selected_event(event, sink)?;
        increment_sequence(sequence)?;
        Ok(())
    }

    pub(crate) fn context(
        &self,
        sequence: u64,
        well: Option<usize>,
        catalyst: Option<Catalyst>,
    ) -> EventContext {
        EventContext {
            arch: MachineId::from(MACHINE_ID),
            machine_version: VersionStamp::from(MACHINE_VERSION),
            run_id: self.request.run_id.clone(),
            sequence,
            step: self.image.reactor.sweeps,
            cycle_or_tick: Some(self.image.reactor.sweeps),
            frame: Some(self.image.reactor.frames),
            pc: well.map(|well| Address::new("well", well as u64)),
            instruction: catalyst.map(|catalyst| InstructionRef {
                encoding: "hexwell.catalyst8".into(),
                bytes: vec![catalyst.byte],
                decoded: Some(catalyst.disassemble()),
            }),
        }
    }

    pub(crate) fn normalize_native_event(
        &self,
        context: &EventContext,
        kind: &str,
        payload: Value,
    ) -> Result<ExecutionEvent, String> {
        let native = NativeEvent {
            schema: schema("hexwell.event"),
            kind: kind.into(),
            payload,
        };
        let mut events = HexwellNativeEventAdapter.normalize(context, &native)?;
        if events.len() != 1 {
            return Err(format!(
                "Hexwell native-event adapter returned {} events for {kind:?}; expected one",
                events.len()
            ));
        }
        let mut event = events.remove(0);
        if !self.request.observation.native_evidence.enabled
            || !self.request.observation.native_evidence.includes(kind)
        {
            event.extensions.clear();
        }
        Ok(event)
    }

    pub(crate) fn emit_sweep(
        &mut self,
        outcome: &SweepOutcome,
        sequence: &mut u64,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        let fired_kind = EventKind::Extension("hexwell.catalyst_fired".into());
        if self
            .request
            .observation
            .normalized_events
            .events
            .includes(&fired_kind)
        {
            for firing in &outcome.firings {
                let context = EventContext {
                    step: outcome.sweep,
                    cycle_or_tick: Some(outcome.sweep),
                    ..self.context(*sequence, Some(firing.well), Some(firing.catalyst))
                };
                let native_payload = json!({
                    "family": firing.catalyst.family.mnemonic().to_ascii_lowercase(),
                    "arg": firing.catalyst.arg,
                    "materia": firing.catalyst.materia().state_name(),
                    "spark_targets": firing.spark_targets,
                    "commit": "simultaneous_at_sweep_end"
                });
                let mut event =
                    self.normalize_native_event(&context, "catalyst", native_payload.clone())?;
                if self.request.observation.native_evidence.enabled
                    && self
                        .request
                        .observation
                        .native_evidence
                        .includes("catalyst")
                {
                    event
                        .extensions
                        .insert("hexwell.catalyst".into(), native_payload);
                }
                self.emit_selected_event(event, sink)?;
                increment_sequence(sequence)?;
            }
        }

        let commit_kind = EventKind::Extension("hexwell.sweep_committed".into());
        if self
            .request
            .observation
            .normalized_events
            .events
            .includes(&commit_kind)
        {
            let context = EventContext {
                step: outcome.sweep,
                cycle_or_tick: Some(outcome.sweep),
                ..self.context(*sequence, None, None)
            };
            let native_payload = json!({
                "sweep": outcome.sweep,
                "active_before": outcome.active_before,
                "active_after": outcome.active_after,
                "matter_before": outcome.matter_before,
                "matter_after": outcome.matter_after,
                "precipitations": outcome.precipitations,
                "dissolutions": outcome.dissolutions,
                "bindings": outcome.bindings,
                "cleavages": outcome.cleavages,
                "tinctures": outcome.tinctures,
                "vented": outcome.vented,
                "transfers": outcome.transfers.iter().map(|transfer| {
                    json!({
                        "from": transfer.from,
                        "to": transfer.to,
                        "materia": transfer.materia.state_name(),
                        "requested": transfer.requested,
                        "accepted": transfer.accepted
                    })
                }).collect::<Vec<_>>()
            });
            let mut event =
                self.normalize_native_event(&context, "sweep", native_payload.clone())?;
            append_change_writes(&self.request, &mut event, &outcome.changes);
            event.io.clear();
            event.io.push(IoObservation {
                port: "reaction_out".into(),
                direction: IoDirection::Output,
                channel: IoChannel::Extension("reaction_commit".into()),
                value: json!({
                    "sweep": outcome.sweep,
                    "active_before": outcome.active_before,
                    "active_after": outcome.active_after,
                    "transfers": outcome.transfers.len(),
                    "reactions": outcome.precipitations
                        + outcome.dissolutions
                        + outcome.bindings
                        + outcome.cleavages
                        + outcome.tinctures,
                    "vented": outcome.vented
                }),
            });
            event.io.push(IoObservation {
                port: "matter_out".into(),
                direction: IoDirection::Output,
                channel: IoChannel::Extension("matter_ledger".into()),
                value: json!({
                    "before": outcome.matter_before,
                    "after": outcome.matter_after,
                    "vented": outcome.vented
                }),
            });
            if self.request.observation.native_evidence.enabled
                && self.request.observation.native_evidence.includes("sweep")
            {
                event
                    .extensions
                    .insert("hexwell.sweep".into(), native_payload);
            }
            self.emit_selected_event(event, sink)?;
            increment_sequence(sequence)?;
        }
        Ok(())
    }

    pub(crate) fn emit_frame(
        &mut self,
        cooling: &[CellChange],
        frame_hash: Option<u64>,
        sequence: &mut u64,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        let kind = EventKind::FrameCompleted;
        let frame_sequence = *sequence;
        if self
            .request
            .observation
            .normalized_events
            .events
            .includes(&kind)
        {
            let context = EventContext {
                frame: Some(self.image.reactor.frames - 1),
                ..self.context(*sequence, None, None)
            };
            let native_payload = json!({
                "cooled_wells": cooling.len(),
                "active_wells": self.image.reactor.active_count(),
                "matter": self.image.reactor.total_matter()
            });
            let mut event =
                self.normalize_native_event(&context, "frame", native_payload.clone())?;
            append_change_writes(&self.request, &mut event, cooling);
            event.io.clear();
            event.io.push(IoObservation {
                port: "reactor_out".into(),
                direction: IoDirection::Output,
                channel: IoChannel::Display,
                value: json!({
                    "format": "rgb332",
                    "width": GRID_WIDTH,
                    "height": GRID_HEIGHT,
                    "phenotype_hash": frame_hash
                }),
            });
            if self.request.observation.native_evidence.enabled
                && self.request.observation.native_evidence.includes("frame")
            {
                event
                    .extensions
                    .insert("hexwell.frame".into(), native_payload);
            }
            self.emit_selected_event(event, sink)?;
            increment_sequence(sequence)?;
        }
        let frame = self.image.reactor.frames.saturating_sub(1);
        let artifact = match self.request.observation.frames.capture {
            FrameCapture::None => None,
            FrameCapture::Hashes => frame_hash.map(|hash| {
                FrameArtifact::fingerprint(
                    self.request.run_id.clone(),
                    MachineId::from(MACHINE_ID),
                    VersionStamp::from(MACHINE_VERSION),
                    schema("hexwell.phenotype_fingerprint"),
                    frame_sequence,
                    self.image.reactor.sweeps,
                    frame,
                    hash.to_le_bytes().to_vec(),
                )
            }),
            FrameCapture::Full => Some(FrameArtifact::full(
                self.request.run_id.clone(),
                MachineId::from(MACHINE_ID),
                VersionStamp::from(MACHINE_VERSION),
                schema("hexwell.rgb332"),
                frame_sequence,
                self.image.reactor.sweeps,
                frame,
                self.image.reactor.render_rgb332(),
            )),
        };
        if let Some(artifact) = artifact {
            sink.emit(Emission::Frame(&artifact))
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub(crate) fn capture_frame(&mut self) -> Option<u64> {
        match self.request.observation.frames.capture {
            FrameCapture::None => None,
            FrameCapture::Hashes | FrameCapture::Full => Some(self.image.reactor.phenotype_hash()),
        }
    }

    pub(crate) fn emit_snapshot(
        &self,
        sequence: u64,
        lifecycle: SessionLifecycle,
        sink: &mut dyn EmissionSink,
    ) -> Result<SnapshotArtifact, String> {
        let snapshot = SnapshotArtifact::new_typed(
            self.request.run_id.clone(),
            MachineId::from(MACHINE_ID),
            VersionStamp::from(MACHINE_VERSION),
            session_snapshot_schema(),
            sequence,
            self.image.reactor.sweeps,
            self.snapshot_bytes_for(lifecycle)?,
        );
        sink.emit(Emission::Snapshot(&snapshot))
            .map_err(|error| error.to_string())?;
        Ok(snapshot)
    }

    pub(crate) fn emit_selected_event(
        &mut self,
        event: ExecutionEvent,
        sink: &mut dyn EmissionSink,
    ) -> Result<(), String> {
        sink.emit(Emission::Event(&event))
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[doc(hidden)]
pub fn increment_sequence(sequence: &mut u64) -> Result<(), String> {
    *sequence = sequence
        .checked_add(1)
        .ok_or_else(|| "Hexwell event-sequence counter exhausted u64".to_string())?;
    Ok(())
}

pub(crate) fn access_before(request: &ExecutionRequest, value: Value) -> Option<Value> {
    match request.observation.normalized_events.access_detail {
        AccessDetail::BeforeAndAfter => Some(value),
        AccessDetail::None | AccessDetail::AfterOnly => None,
    }
}

pub(crate) fn access_after(request: &ExecutionRequest, value: Value) -> Option<Value> {
    match request.observation.normalized_events.access_detail {
        AccessDetail::None => None,
        AccessDetail::AfterOnly | AccessDetail::BeforeAndAfter => Some(value),
    }
}

pub(crate) fn append_change_writes(
    request: &ExecutionRequest,
    event: &mut ExecutionEvent,
    changes: &[CellChange],
) {
    if !request.observation.normalized_events.state_diffs {
        return;
    }
    for change in changes {
        for (name, width, before, after) in [
            (
                "brine",
                4,
                json!(change.before.brine),
                json!(change.after.brine),
            ),
            (
                "ember",
                4,
                json!(change.before.ember),
                json!(change.after.ember),
            ),
            (
                "crystal",
                4,
                json!(change.before.crystal),
                json!(change.after.crystal),
            ),
            (
                "heat",
                4,
                json!(change.before.heat),
                json!(change.after.heat),
            ),
            (
                "spark",
                1,
                json!(change.before.spark),
                json!(change.after.spark),
            ),
        ] {
            if before == after {
                continue;
            }
            event.writes.push(StateWrite {
                location: StateLocation {
                    space: StateSpace::Extension("hexwell.field".into()),
                    name: Some(name.into()),
                    address: Some(Address::new("well", change.well as u64)),
                    width_bits: Some(width),
                },
                before: access_before(request, before),
                after: access_after(request, after),
            });
        }
    }
}
