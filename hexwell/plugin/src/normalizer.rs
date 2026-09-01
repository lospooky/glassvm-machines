use std::collections::BTreeMap;

use glassvm_core::{
    CapabilityDependency, CapabilityDescriptor, CapabilityId, CapabilityOutput, CapabilityReceipt,
    CapabilityRequest, CapabilityStatus, CostClass, Emission, EventKind, ExecutionEvent, InputId,
    InputValueSelector, IoChannel, IoDirection, Normalizer, NormalizerCatalog, NormalizerError,
    NormalizerOutput, SnapshotArtifact, canonical_json_bytes, standard_capabilities,
};
use serde_json::{Value, json};

pub(crate) const REACTION_FIELD_MOTIFS: &str = "hexwell.reaction_field_motifs";
pub(crate) const INPUT_SUMMARY: &str = "hexwell.input_summary";

#[derive(Debug, Default)]
struct InputSummaryState {
    values: u64,
}

impl InputSummaryState {
    fn observe(&mut self, _evidence: &glassvm_core::InputValueEvidence) {
        self.values = self.values.saturating_add(1);
    }

    fn value(&self) -> Value {
        json!({"values": self.values})
    }
}

#[derive(Debug, Default)]
struct ReactionFieldMotifState {
    sweep_events: u64,
    catalyst_events: u64,
    frame_events: u64,
    feed_events: u64,
    input_events: u64,
    reaction_count: u64,
    transfer_count: u64,
    vented_matter: u64,
    max_active_wells: u64,
    active_rise_events: u64,
    active_fall_events: u64,
    matter_initial: Option<u64>,
    matter_final: Option<u64>,
}

impl ReactionFieldMotifState {
    fn observe(&mut self, event: &ExecutionEvent) {
        match &event.kind {
            EventKind::Extension(kind) if kind == "hexwell.sweep_committed" => {
                self.sweep_events = self.sweep_events.saturating_add(1);
                if let Some(value) = io_value(event, "reaction_commit") {
                    self.reaction_count = self
                        .reaction_count
                        .saturating_add(u64_field(value, "reactions").unwrap_or(0));
                    self.transfer_count = self
                        .transfer_count
                        .saturating_add(u64_field(value, "transfers").unwrap_or(0));
                    self.vented_matter = self
                        .vented_matter
                        .saturating_add(u64_field(value, "vented").unwrap_or(0));
                    let active_before = u64_field(value, "active_before").unwrap_or(0);
                    let active_after = u64_field(value, "active_after").unwrap_or(0);
                    self.max_active_wells = self.max_active_wells.max(active_after);
                    if active_after > active_before {
                        self.active_rise_events = self.active_rise_events.saturating_add(1);
                    } else if active_after < active_before {
                        self.active_fall_events = self.active_fall_events.saturating_add(1);
                    }
                }
                if let Some(value) = io_value(event, "matter_ledger") {
                    let before = u64_field(value, "before");
                    let after = u64_field(value, "after");
                    if self.matter_initial.is_none() {
                        self.matter_initial = before;
                    }
                    self.matter_final = after;
                }
            }
            EventKind::Extension(kind) if kind == "hexwell.catalyst_fired" => {
                self.catalyst_events = self.catalyst_events.saturating_add(1);
            }
            EventKind::Extension(kind) if kind == "hexwell.tide_feed" => {
                self.feed_events = self.feed_events.saturating_add(1);
            }
            EventKind::InputSampled => {
                self.input_events = self.input_events.saturating_add(1);
            }
            EventKind::FrameCompleted => {
                self.frame_events = self.frame_events.saturating_add(1);
            }
            _ => {}
        }
    }

    fn value(&self) -> Value {
        let matter_change = match (self.matter_initial, self.matter_final) {
            (Some(initial), Some(final_value)) if final_value >= initial => {
                json!(final_value - initial)
            }
            (Some(initial), Some(final_value)) => json!(
                i64::try_from(initial - final_value)
                    .map(|magnitude| -magnitude)
                    .unwrap_or(i64::MIN)
            ),
            _ => Value::Null,
        };
        json!({
            "schema": "hexwell_reaction_field_motifs",
            "sweep_events": self.sweep_events,
            "catalyst_events": self.catalyst_events,
            "frame_events": self.frame_events,
            "feed_events": self.feed_events,
            "input_events": self.input_events,
            "reaction_count": self.reaction_count,
            "transfer_count": self.transfer_count,
            "vented_matter": self.vented_matter,
            "max_active_wells": self.max_active_wells,
            "active_rise_events": self.active_rise_events,
            "active_fall_events": self.active_fall_events,
            "matter_initial": self.matter_initial,
            "matter_final": self.matter_final,
            "matter_change": matter_change,
        })
    }
}

fn io_value<'a>(event: &'a ExecutionEvent, channel: &str) -> Option<&'a Value> {
    event.io.iter().find_map(|observation| {
        matches!(
            (&observation.direction, &observation.channel),
            (IoDirection::Output, IoChannel::Extension(name)) if name == channel
        )
        .then_some(&observation.value)
    })
}

fn u64_field(value: &Value, field: &str) -> Option<u64> {
    value.as_object()?.get(field)?.as_u64()
}

pub(crate) struct HexwellNormalizer {
    requests: Vec<CapabilityRequest>,
    native_outputs: BTreeMap<CapabilityId, CapabilityOutput>,
    reaction_requested: bool,
    reaction: ReactionFieldMotifState,
    input_summary: InputSummaryState,
}

impl HexwellNormalizer {
    pub(crate) fn new() -> Self {
        Self {
            requests: Vec::new(),
            native_outputs: BTreeMap::new(),
            reaction_requested: false,
            reaction: ReactionFieldMotifState::default(),
            input_summary: InputSummaryState::default(),
        }
    }
}

impl Normalizer for HexwellNormalizer {
    fn begin(&mut self, requests: &[CapabilityRequest]) -> Result<(), NormalizerError> {
        self.requests = requests.to_vec();
        self.native_outputs.clear();
        self.reaction_requested = requests
            .iter()
            .any(|request| request.id.as_str() == REACTION_FIELD_MOTIFS);
        self.reaction = ReactionFieldMotifState::default();
        self.input_summary = InputSummaryState::default();
        Ok(())
    }

    fn observe(&mut self, emission: &Emission<'_>) -> Result<(), NormalizerError> {
        if self.reaction_requested
            && let Emission::Event(event) = emission
        {
            self.reaction.observe(event);
        }
        if let Emission::NativeEvidence(evidence) = emission {
            let id = CapabilityId::new(&evidence.native_event.kind).map_err(|_| {
                NormalizerError::new(format!(
                    "Hexwell native evidence kind is not a canonical capability ID: {}",
                    evidence.native_event.kind
                ))
            })?;
            if self.requests.iter().any(|request| request.id == id) {
                self.native_outputs.insert(
                    id.clone(),
                    CapabilityOutput {
                        schema: id.schema(),
                        value: evidence.native_event.payload.clone(),
                    },
                );
            }
        }
        if let Emission::InputValueEvidence(evidence) = emission
            && self
                .requests
                .iter()
                .any(|request| request.id.as_str() == INPUT_SUMMARY)
        {
            self.input_summary.observe(evidence);
        }
        Ok(())
    }

    fn finalize(
        &mut self,
        final_state: Option<&SnapshotArtifact>,
    ) -> Result<NormalizerOutput, NormalizerError> {
        let _ = final_state;
        let mut capabilities = Vec::new();
        let mut receipts = Vec::new();
        for request in &self.requests {
            let output = if request.id.as_str() == REACTION_FIELD_MOTIFS {
                Some(CapabilityOutput {
                    schema: request.id.schema(),
                    value: self.reaction.value(),
                })
            } else if request.id.as_str() == INPUT_SUMMARY {
                Some(CapabilityOutput {
                    schema: request.id.schema(),
                    value: self.input_summary.value(),
                })
            } else {
                self.native_outputs.get(&request.id).cloned()
            };
            let Some(output) = output else {
                receipts.push(CapabilityReceipt {
                    id: request.id.clone(),
                    status: if request.required {
                        CapabilityStatus::Failed
                    } else {
                        CapabilityStatus::Unavailable
                    },
                    output_schema: None,
                    logical_bytes: 0,
                    message: Some("requested native evidence produced no capability output".into()),
                });
                continue;
            };
            let logical_bytes = canonical_json_bytes(&output.value)
                .map_err(NormalizerError::new)?
                .len() as u64;
            receipts.push(CapabilityReceipt {
                id: request.id.clone(),
                status: CapabilityStatus::Fulfilled,
                output_schema: Some(output.schema.clone()),
                logical_bytes,
                message: None,
            });
            capabilities.push(output);
        }
        Ok(NormalizerOutput {
            capabilities,
            receipts,
        })
    }
}

fn capability(id: &str, cost_class: CostClass) -> CapabilityDescriptor {
    CapabilityDescriptor {
        schema: CapabilityId::new(id)
            .expect("static Hexwell capability ID")
            .schema(),
        output_type: "json.object".into(),
        dependencies: vec![CapabilityDependency::NativeEvidence {
            schema_id: "hexwell.observation".into(),
        }],
        cost_class,
    }
}

fn normalized_capability(id: &str, cost_class: CostClass) -> CapabilityDescriptor {
    let dependencies = if id == REACTION_FIELD_MOTIFS {
        vec![
            CapabilityDependency::NormalizedEvents,
            normalized_event_kinds(&[
                "extension:hexwell.sweep_committed",
                "extension:hexwell.catalyst_fired",
                "extension:hexwell.tide_feed",
                "input_sampled",
                "frame_completed",
            ]),
        ]
    } else {
        vec![
            CapabilityDependency::NormalizedEvents,
            normalized_event_kinds(&[
                "instruction_decoded",
                "branch_taken",
                "call",
                "return",
                "interrupt",
                "trap",
            ]),
        ]
    };
    CapabilityDescriptor {
        schema: CapabilityId::new(id)
            .expect("static Hexwell capability ID")
            .schema(),
        output_type: "json.object".into(),
        dependencies,
        cost_class,
    }
}

fn normalized_event_kinds(kinds: &[&str]) -> CapabilityDependency {
    CapabilityDependency::NormalizedEventKinds {
        kinds: kinds.iter().map(|kind| (*kind).into()).collect(),
    }
}

pub fn catalog() -> NormalizerCatalog {
    NormalizerCatalog {
        schema_version: glassvm_core::NORMALIZER_CONTRACT_SCHEMA_VERSION,
        normalizer_id: "hexwell.normalizer".into(),
        normalizer_version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            capability("hexwell.framebuffer", CostClass::Bounded),
            capability("hexwell.reactor_state", CostClass::Bounded),
            capability("hexwell.reaction_dynamics", CostClass::Linear),
            normalized_capability(
                standard_capabilities::CONTROL_FLOW_MOTIFS,
                CostClass::Bounded,
            ),
            normalized_capability(REACTION_FIELD_MOTIFS, CostClass::Bounded),
            CapabilityDescriptor {
                schema: CapabilityId::new(INPUT_SUMMARY)
                    .expect("static Hexwell input-summary ID")
                    .schema(),
                output_type: "json.object".into(),
                dependencies: vec![CapabilityDependency::InputValueEvidence {
                    selectors: vec![InputValueSelector::InputId(
                        InputId::new("hexwell.tide").expect("static Hexwell input ID"),
                    )],
                }],
                cost_class: CostClass::Bounded,
            },
        ],
    }
}
