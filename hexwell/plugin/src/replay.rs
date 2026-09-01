//! Hexwell: Deterministic scheduled input and replay-result materialization.

use glassvm_core::{
    CapabilityOutput, CommonMetrics, Emission, EmissionSink, EventKind, InputApplied,
    InputCoordinate, InputId, InputSource, InputValueEvidence, IoChannel, IoDirection,
    IoObservation, NativeEvent, NativeEvidenceEnvelope, NormalizerOutput, RunResult, StateLocation,
    StateSpace, StateWrite, StructuredValue, TypedInputPayload,
};
use serde_json::json;

use crate::emulator_session::HexwellSession;
use crate::identity::{MACHINE_ID, MACHINE_VERSION, schema};
use crate::trace::{access_after, access_before, append_change_writes, increment_sequence};
use hexwell_core::{FeedOutcome, GRID_HEIGHT, GRID_WIDTH, WELL_COUNT, connected_matter_components};

impl HexwellSession {
    pub(crate) fn apply_scheduled_inputs(
        &mut self,
        sequence: &mut u64,
        mut sink: Option<&mut dyn EmissionSink>,
    ) -> Result<(), String> {
        while let Some(scheduled_input) = self.scheduled_inputs.get(self.image.next_scheduled_input)
        {
            let frame = scheduled_input.frame;
            if frame > self.image.reactor.frames {
                break;
            }
            if frame < self.image.reactor.frames {
                return Err(format!(
                    "Hexwell stimulus {} at frame {frame} was not applied before frame {}",
                    scheduled_input.ordinal, self.image.reactor.frames
                ));
            }
            let tide = scheduled_input.value;
            let before = self.image.reactor.tide;
            self.image.reactor.set_tide(tide);

            let input_id = InputId::new("hexwell.tide").expect("static Hexwell input ID");
            let input_schema = schema("hexwell.input.tide");
            let source = InputSource::Scheduled {
                ordinal: scheduled_input.ordinal,
            };
            let application_coordinate = InputCoordinate::frame(frame);

            let kind = EventKind::InputApplied;
            if let Some(sink) = sink.as_deref_mut() {
                if self
                    .request
                    .observation
                    .normalized_events
                    .events
                    .includes(&kind)
                {
                    let context = self.context(*sequence, None, None);
                    let native_payload = json!({"mask": tide, "ordinal": scheduled_input.ordinal});
                    let mut event =
                        self.normalize_native_event(&context, "tide", native_payload)?;
                    event.extensions.insert(
                        "glassvm.input_applied".into(),
                        serde_json::to_value(InputApplied {
                            input_id: input_id.clone(),
                            input_schema: input_schema.clone(),
                            source,
                            application_coordinate: application_coordinate.clone(),
                        })
                        .map_err(|error| format!("encode Hexwell InputApplied: {error}"))?,
                    );
                    if self.request.observation.normalized_events.state_diffs {
                        event.writes.push(StateWrite {
                            location: StateLocation {
                                space: StateSpace::Input,
                                name: Some("tide".into()),
                                address: None,
                                width_bits: Some(8),
                            },
                            before: access_before(&self.request, json!(before)),
                            after: access_after(&self.request, json!(tide)),
                        });
                    }
                    event.io.push(IoObservation {
                        port: "tide_in".into(),
                        direction: IoDirection::Input,
                        channel: IoChannel::Extension("matter_flux".into()),
                        value: json!({"input_id": input_id, "ordinal": scheduled_input.ordinal}),
                    });
                    self.emit_selected_event(event, sink)?;
                    increment_sequence(sequence)?;
                }
                if self
                    .prepared_observation
                    .input_value_ids
                    .iter()
                    .any(|selected| selected == &input_id)
                {
                    let payload = TypedInputPayload::new(
                        input_schema.clone(),
                        StructuredValue::Unsigned(u64::from(tide)),
                    )?;
                    let evidence = InputValueEvidence::new(
                        input_id,
                        input_schema,
                        source,
                        application_coordinate,
                        payload,
                    );
                    sink.emit(Emission::InputValueEvidence(&evidence))
                        .map_err(|error| error.to_string())?;
                }
            }
            self.image.next_scheduled_input += 1;
        }
        Ok(())
    }

    pub(crate) fn apply_feed(
        &mut self,
        sequence: &mut u64,
        sink: Option<&mut dyn EmissionSink>,
    ) -> Result<FeedOutcome, String> {
        let mut reactor = self.image.reactor.clone();
        let outcome = reactor.feed_tide();
        let introduced_matter = self
            .image
            .telemetry
            .introduced_matter
            .checked_add(outcome.introduced)
            .ok_or_else(|| "Hexwell introduced-matter ledger exhausted u64".to_string())?;
        self.image.reactor = reactor;
        self.image.telemetry.introduced_matter = introduced_matter;
        let kind = EventKind::Extension("hexwell.tide_feed".into());
        if self
            .request
            .observation
            .normalized_events
            .events
            .includes(&kind)
            && let Some(sink) = sink
        {
            let context = self.context(*sequence, None, None);
            let native_payload = json!({
                "introduced": outcome.introduced,
                "ignited": outcome.ignited,
                "changed_wells": outcome.changes.iter().map(|change| change.well).collect::<Vec<_>>()
            });
            let mut event =
                self.normalize_native_event(&context, "feed", native_payload.clone())?;
            append_change_writes(&self.request, &mut event, &outcome.changes);
            event.io.clear();
            event.io.push(IoObservation {
                port: "tide_in".into(),
                direction: IoDirection::Input,
                channel: IoChannel::Extension("matter_flux".into()),
                value: json!({
                    "mask": self.image.reactor.tide,
                    "introduced": outcome.introduced,
                    "ignited": outcome.ignited
                }),
            });
            event.io.push(IoObservation {
                port: "matter_out".into(),
                direction: IoDirection::Output,
                channel: IoChannel::Extension("matter_ledger".into()),
                value: json!({
                    "current": self.image.reactor.total_matter(),
                    "introduced": self.image.telemetry.introduced_matter,
                    "vented": self.image.telemetry.vented_matter
                }),
            });
            if self.request.observation.native_evidence.enabled
                && self.request.observation.native_evidence.includes("feed")
            {
                event
                    .extensions
                    .insert("hexwell.feed".into(), native_payload);
            }
            self.emit_selected_event(event, sink)?;
            increment_sequence(sequence)?;
        }
        Ok(outcome)
    }

    pub(crate) fn observations(&self, termination: &str) -> Vec<NativeEvidenceEnvelope> {
        let reactor = &self.image.reactor;
        let telemetry = &self.image.telemetry;
        let framebuffer = reactor.render_rgb332();
        let occupied_wells = (0..WELL_COUNT)
            .filter(|well| reactor.brine[*well] + reactor.ember[*well] + reactor.crystal[*well] > 0)
            .count();
        let families_reached = telemetry
            .opcode_counts
            .iter()
            .filter(|count| **count > 0)
            .count();
        let matter = reactor.total_matter();
        let invariant_error = matter as i64 - telemetry.expected_matter() as i64;
        let spark_wells = reactor
            .sparks
            .iter()
            .enumerate()
            .filter_map(|(well, spark)| (*spark).then_some(well))
            .collect::<Vec<_>>();
        let envelope = |sequence: u64, kind: &str, payload: serde_json::Value| {
            NativeEvidenceEnvelope::new(
                self.request.run_id.clone(),
                MACHINE_ID.into(),
                MACHINE_VERSION.into(),
                sequence,
                reactor.sweeps,
                NativeEvent {
                    schema: schema("hexwell.observation"),
                    kind: kind.into(),
                    payload,
                },
            )
        };
        vec![
            envelope(
                0,
                "hexwell.framebuffer",
                json!({
                    "width": GRID_WIDTH,
                    "height": GRID_HEIGHT,
                    "planes": 1,
                    "format": "rgb332",
                    "bytes": framebuffer
                }),
            ),
            envelope(
                1,
                "hexwell.reactor_state",
                json!({
                    "width": GRID_WIDTH,
                    "height": GRID_HEIGHT,
                    "topology": "odd-r-hex-torus-v1",
                    "brine": reactor.brine,
                    "ember": reactor.ember,
                    "crystal": reactor.crystal,
                    "heat": reactor.heat,
                    "spark_wells": spark_wells,
                    "tide": reactor.tide
                }),
            ),
            envelope(
                2,
                "hexwell.reaction_dynamics",
                json!({
                    "sweeps": reactor.sweeps,
                    "firings": reactor.firings,
                    "active_wells": reactor.active_count(),
                    "peak_active_wells": telemetry.peak_active_wells,
                    "occupied_wells": occupied_wells,
                    "matter_components": connected_matter_components(reactor),
                    "matter_initial": telemetry.initial_matter,
                    "matter_introduced": telemetry.introduced_matter,
                    "matter_vented": telemetry.vented_matter,
                    "matter_current": matter,
                    "matter_invariant_error": invariant_error,
                    "transfer_requests": telemetry.transfer_requests,
                    "transfer_quanta_requested": telemetry.transfer_quanta_requested,
                    "transfer_quanta_accepted": telemetry.transfer_quanta_accepted,
                    "precipitations": telemetry.precipitations,
                    "dissolutions": telemetry.dissolutions,
                    "bindings": telemetry.bindings,
                    "cleavages": telemetry.cleavages,
                    "tinctures": telemetry.tinctures,
                    "vents": telemetry.vents,
                    "opcode_counts": telemetry.opcode_counts,
                    "families_reached": families_reached,
                    "termination": termination
                }),
            ),
        ]
    }

    pub(crate) fn result(
        &self,
        termination: &str,
        normalized: &[CapabilityOutput],
    ) -> Result<RunResult, String> {
        let reactor = &self.image.reactor;
        Ok(RunResult {
            common: CommonMetrics {
                cycles: reactor.sweeps,
                frames: reactor.frames,
                termination: termination.into(),
                boot_success: true,
            },
            capabilities: normalized.to_vec(),
        })
    }
}

pub(crate) fn normalized_capabilities(output: &NormalizerOutput) -> Vec<CapabilityOutput> {
    output.capabilities.clone()
}
