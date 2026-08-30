use std::collections::BTreeMap;

use glassvm_core::{
    CapabilityDependency, CapabilityDescriptor, CapabilityId, CapabilityOutput, CapabilityReceipt,
    CapabilityRequest, CapabilityStatus, CostClass, Emission, EventKind, ExecutionEvent,
    FrameCaptureRequirement, IoChannel, IoDirection, Normalizer, NormalizerCatalog,
    NormalizerError, NormalizerOutput, SnapshotArtifact, StateSpace, canonical_json_bytes,
    standard_capabilities,
};
use serde_json::{Value, json};

pub(crate) const VISUAL_TRAJECTORY_MOTIFS: &str = "chip8.visual_trajectory_motifs";

#[derive(Debug, Default)]
struct VisualTrajectoryMotifState {
    frame_events: u64,
    frame_hash_observations: u64,
    frame_hash_changes: u64,
    repeated_frame_hashes: u64,
    full_frame_observations: u64,
    display_io_events: u64,
    display_write_events: u64,
    display_state_writes: u64,
    max_display_writes_per_event: u64,
    input_events: u64,
    last_frame_hash: Option<Value>,
}

impl VisualTrajectoryMotifState {
    fn observe(&mut self, event: &ExecutionEvent) {
        match &event.kind {
            EventKind::FrameCompleted => {
                self.frame_events = self.frame_events.saturating_add(1);
                if let Some(value) = display_io_value(event) {
                    self.display_io_events = self.display_io_events.saturating_add(1);
                    if value
                        .as_object()
                        .is_some_and(|object| object.contains_key("bytes"))
                    {
                        self.full_frame_observations =
                            self.full_frame_observations.saturating_add(1);
                    }
                    if let Some(frame_hash) = value
                        .as_object()
                        .and_then(|object| object.get("frame_hash"))
                    {
                        self.frame_hash_observations =
                            self.frame_hash_observations.saturating_add(1);
                        if self.last_frame_hash.as_ref() == Some(frame_hash) {
                            self.repeated_frame_hashes =
                                self.repeated_frame_hashes.saturating_add(1);
                        } else if self.last_frame_hash.is_some() {
                            self.frame_hash_changes = self.frame_hash_changes.saturating_add(1);
                        }
                        self.last_frame_hash = Some(frame_hash.clone());
                    }
                }
            }
            EventKind::DisplayWrite => {
                self.display_write_events = self.display_write_events.saturating_add(1);
                let display_writes = event
                    .writes
                    .iter()
                    .filter(|write| write.location.space == StateSpace::Display)
                    .count() as u64;
                self.display_state_writes =
                    self.display_state_writes.saturating_add(display_writes);
                self.max_display_writes_per_event =
                    self.max_display_writes_per_event.max(display_writes);
            }
            EventKind::InputSampled => {
                self.input_events = self.input_events.saturating_add(1);
            }
            _ => {}
        }
    }

    fn value(&self) -> Value {
        json!({
            "schema": "chip8_visual_trajectory_motifs",
            "frame_events": self.frame_events,
            "frame_hash_observations": self.frame_hash_observations,
            "frame_hash_changes": self.frame_hash_changes,
            "repeated_frame_hashes": self.repeated_frame_hashes,
            "full_frame_observations": self.full_frame_observations,
            "display_io_events": self.display_io_events,
            "display_write_events": self.display_write_events,
            "display_state_writes": self.display_state_writes,
            "max_display_writes_per_event": self.max_display_writes_per_event,
            "input_events": self.input_events,
        })
    }
}

fn display_io_value<'a>(event: &'a ExecutionEvent) -> Option<&'a Value> {
    event.io.iter().find_map(|observation| {
        matches!(
            (&observation.direction, &observation.channel),
            (IoDirection::Output, IoChannel::Display)
        )
        .then_some(&observation.value)
    })
}

pub(crate) struct Chip8Normalizer {
    requests: Vec<CapabilityRequest>,
    native_outputs: BTreeMap<CapabilityId, CapabilityOutput>,
    visual_requested: bool,
    visual: VisualTrajectoryMotifState,
}

impl Chip8Normalizer {
    pub(crate) fn new() -> Self {
        Self {
            requests: Vec::new(),
            native_outputs: BTreeMap::new(),
            visual_requested: false,
            visual: VisualTrajectoryMotifState::default(),
        }
    }
}

impl Normalizer for Chip8Normalizer {
    fn begin(&mut self, requests: &[CapabilityRequest]) -> Result<(), NormalizerError> {
        self.requests = requests.to_vec();
        self.native_outputs.clear();
        self.visual_requested = requests
            .iter()
            .any(|request| request.id.as_str() == VISUAL_TRAJECTORY_MOTIFS);
        self.visual = VisualTrajectoryMotifState::default();
        Ok(())
    }

    fn observe(&mut self, emission: &Emission<'_>) -> Result<(), NormalizerError> {
        if self.visual_requested
            && let Emission::Event(event) = emission
        {
            self.visual.observe(event);
        }
        if let Emission::NativeEvidence(evidence) = emission {
            let id = CapabilityId::new(&evidence.native_event.kind).map_err(|_| {
                NormalizerError::new(format!(
                    "CHIP-8 native evidence kind is not a canonical capability ID: {}",
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
            let output = if request.id.as_str() == VISUAL_TRAJECTORY_MOTIFS {
                Some(CapabilityOutput {
                    schema: request.id.schema(),
                    value: self.visual.value(),
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

fn capability(
    id: &str,
    dependencies: Vec<CapabilityDependency>,
    cost_class: CostClass,
) -> CapabilityDescriptor {
    CapabilityDescriptor {
        schema: CapabilityId::new(id)
            .expect("static CHIP-8 capability ID")
            .schema(),
        output_type: "json.object".into(),
        dependencies,
        cost_class,
    }
}

fn normalized_capability(
    id: &str,
    cost_class: CostClass,
    dependencies: Vec<CapabilityDependency>,
) -> CapabilityDescriptor {
    CapabilityDescriptor {
        schema: CapabilityId::new(id)
            .expect("static CHIP-8 capability ID")
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
        normalizer_id: "chip8.normalizer".into(),
        normalizer_version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            capability(
                "chip8.framebuffer",
                vec![CapabilityDependency::NativeEvidence {
                    schema_id: "chip8.observation".into(),
                }],
                CostClass::Bounded,
            ),
            capability(
                "chip8.display_dims",
                vec![CapabilityDependency::NativeEvidence {
                    schema_id: "chip8.observation".into(),
                }],
                CostClass::Negligible,
            ),
            capability(
                "chip8.coverage_summary",
                vec![CapabilityDependency::NativeEvidence {
                    schema_id: "chip8.observation".into(),
                }],
                CostClass::Bounded,
            ),
            capability(
                "chip8.execution_summary",
                vec![CapabilityDependency::NativeEvidence {
                    schema_id: "chip8.observation".into(),
                }],
                CostClass::Bounded,
            ),
            capability(
                "chip8.interestingness",
                vec![CapabilityDependency::NativeEvidence {
                    schema_id: "chip8.observation".into(),
                }],
                CostClass::Heavy,
            ),
            capability("chip8.coverage", Vec::new(), CostClass::Linear),
            capability(
                "chip8.frame_hashes",
                vec![CapabilityDependency::Frames {
                    capture: FrameCaptureRequirement::Hashes,
                }],
                CostClass::Bounded,
            ),
            capability("chip8.trajectory_identity", Vec::new(), CostClass::Linear),
            normalized_capability(
                standard_capabilities::CONTROL_FLOW_MOTIFS,
                CostClass::Bounded,
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
                ],
            ),
            normalized_capability(
                VISUAL_TRAJECTORY_MOTIFS,
                CostClass::Bounded,
                vec![
                    CapabilityDependency::NormalizedEvents,
                    normalized_event_kinds(&["frame_completed", "display_write", "input_sampled"]),
                    CapabilityDependency::Frames {
                        capture: FrameCaptureRequirement::Hashes,
                    },
                ],
            ),
        ],
    }
}
