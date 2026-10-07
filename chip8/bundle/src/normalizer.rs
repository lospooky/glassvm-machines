use std::collections::{BTreeMap, BTreeSet};

use glassvm_core::{
    CapabilityDependency, CapabilityDescriptor, CapabilityId, CapabilityOutput, CapabilityReceipt,
    CapabilityRequest, CapabilityStatus, CostClass, Emission, EventKind, ExecutionEvent,
    FrameCaptureRequirement, FrameEvidence, InputValueSelector, Normalizer, NormalizerCatalog,
    NormalizerError, NormalizerOutput, SchemaFamilyId, SnapshotArtifact, StateSpace,
    canonical_json_bytes, standard_capabilities,
};
use serde_json::{Value, json};

pub(crate) const VISUAL_TRAJECTORY_MOTIFS: &str = "chip8.visual_trajectory_motifs";
pub(crate) const INPUT_SUMMARY: &str = "chip8.input_summary";

#[derive(Debug, Default)]
struct InputSummaryState {
    values: u64,
    distinct_inputs: BTreeSet<String>,
}

impl InputSummaryState {
    fn observe(&mut self, evidence: &glassvm_core::InputValueEvidence) {
        self.values = self.values.saturating_add(1);
        self.distinct_inputs
            .insert(evidence.input_id.as_str().to_owned());
    }

    fn value(&self) -> Value {
        json!({
            "values": self.values,
            "distinct_inputs": self.distinct_inputs.len(),
        })
    }
}

#[derive(Debug, Default)]
struct VisualTrajectoryMotifState {
    frame_events: u64,
    frame_artifacts: u64,
    frame_hash_changes: u64,
    repeated_frame_hashes: u64,
    full_artifact_count: u64,
    display_write_events: u64,
    display_state_writes: u64,
    max_display_writes_per_event: u64,
    input_events: u64,
    last_frame_digest: Option<[u8; 32]>,
}

impl VisualTrajectoryMotifState {
    fn observe(&mut self, event: &ExecutionEvent) {
        match &event.kind {
            EventKind::FrameCompleted => {
                self.frame_events = self.frame_events.saturating_add(1);
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

    fn observe_frame(&mut self, frame: &glassvm_core::FrameArtifact) {
        self.frame_artifacts = self.frame_artifacts.saturating_add(1);
        let digest = match &frame.evidence {
            FrameEvidence::Fingerprint { bytes } if bytes.len() == 32 => {
                let mut digest = [0; 32];
                digest.copy_from_slice(bytes);
                digest
            }
            FrameEvidence::Fingerprint { bytes } => {
                glassvm_core::ContentDigest::sha256(bytes).value
            }
            FrameEvidence::Full { bytes } => {
                self.full_artifact_count = self.full_artifact_count.saturating_add(1);
                glassvm_core::ContentDigest::sha256(bytes).value
            }
        };
        if let Some(previous) = self.last_frame_digest {
            if previous == digest {
                self.repeated_frame_hashes = self.repeated_frame_hashes.saturating_add(1);
            } else {
                self.frame_hash_changes = self.frame_hash_changes.saturating_add(1);
            }
        }
        self.last_frame_digest = Some(digest);
    }

    fn value(&self) -> Value {
        json!({
            "schema": "chip8_visual_trajectory_motifs",
            "frame_events": self.frame_events,
            "frame_artifacts": self.frame_artifacts,
            "frame_hash_changes": self.frame_hash_changes,
            "repeated_frame_hashes": self.repeated_frame_hashes,
            "full_artifact_count": self.full_artifact_count,
            "display_write_events": self.display_write_events,
            "display_state_writes": self.display_state_writes,
            "max_display_writes_per_event": self.max_display_writes_per_event,
            "input_events": self.input_events,
        })
    }
}

#[derive(Debug, Default)]
struct ControlFlowMotifState {
    instructions: u64,
    branches: u64,
    calls: u64,
    returns: u64,
    interrupts: u64,
    traps: u64,
    max_call_depth: u64,
    call_depth: u64,
}

impl ControlFlowMotifState {
    fn observe(&mut self, event: &ExecutionEvent) {
        match event.kind {
            EventKind::InstructionDecoded => {
                self.instructions = self.instructions.saturating_add(1)
            }
            EventKind::BranchTaken => self.branches = self.branches.saturating_add(1),
            EventKind::Call => {
                self.calls = self.calls.saturating_add(1);
                self.call_depth = self.call_depth.saturating_add(1);
                self.max_call_depth = self.max_call_depth.max(self.call_depth);
            }
            EventKind::Return => {
                self.returns = self.returns.saturating_add(1);
                self.call_depth = self.call_depth.saturating_sub(1);
            }
            EventKind::Interrupt => self.interrupts = self.interrupts.saturating_add(1),
            EventKind::Trap => self.traps = self.traps.saturating_add(1),
            _ => {}
        }
    }

    fn value(&self) -> Value {
        json!({
            "schema": "bounded_control_flow_motifs",
            "instructions": self.instructions,
            "branches": self.branches,
            "calls": self.calls,
            "returns": self.returns,
            "interrupts": self.interrupts,
            "traps": self.traps,
            "max_call_depth": self.max_call_depth,
            "open_call_depth": self.call_depth,
        })
    }
}

pub(crate) struct Chip8Normalizer {
    requests: Vec<CapabilityRequest>,
    native_outputs: BTreeMap<CapabilityId, CapabilityOutput>,
    control_flow_requested: bool,
    control_flow: ControlFlowMotifState,
    visual_requested: bool,
    visual: VisualTrajectoryMotifState,
    input_summary: InputSummaryState,
}

impl Chip8Normalizer {
    pub(crate) fn new() -> Self {
        Self {
            requests: Vec::new(),
            native_outputs: BTreeMap::new(),
            control_flow_requested: false,
            control_flow: ControlFlowMotifState::default(),
            visual_requested: false,
            visual: VisualTrajectoryMotifState::default(),
            input_summary: InputSummaryState::default(),
        }
    }
}

impl Normalizer for Chip8Normalizer {
    fn begin(&mut self, requests: &[CapabilityRequest]) -> Result<(), NormalizerError> {
        self.requests = requests.to_vec();
        self.native_outputs.clear();
        self.control_flow_requested = requests
            .iter()
            .any(|request| request.id.as_str() == standard_capabilities::CONTROL_FLOW_MOTIFS);
        self.control_flow = ControlFlowMotifState::default();
        self.visual_requested = requests
            .iter()
            .any(|request| request.id.as_str() == VISUAL_TRAJECTORY_MOTIFS);
        self.visual = VisualTrajectoryMotifState::default();
        self.input_summary = InputSummaryState::default();
        Ok(())
    }

    fn observe(&mut self, emission: &Emission<'_>) -> Result<(), NormalizerError> {
        if let Emission::Event(event) = emission {
            if self.control_flow_requested {
                self.control_flow.observe(event);
            }
            if self.visual_requested {
                self.visual.observe(event);
            }
        }
        if self.visual_requested
            && let Emission::Frame(frame) = emission
        {
            self.visual.observe_frame(frame);
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
            let output = if request.id.as_str() == standard_capabilities::CONTROL_FLOW_MOTIFS {
                Some(CapabilityOutput {
                    schema: request.id.schema(),
                    value: self.control_flow.value(),
                })
            } else if request.id.as_str() == VISUAL_TRAJECTORY_MOTIFS {
                Some(CapabilityOutput {
                    schema: request.id.schema(),
                    value: self.visual.value(),
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

fn native_capability(id: &str, cost_class: CostClass) -> CapabilityDescriptor {
    capability(
        id,
        vec![
            CapabilityDependency::NativeEvidence {
                schema_id: id.into(),
            },
            CapabilityDependency::NativeEventKinds {
                kinds: vec![id.into()],
            },
        ],
        cost_class,
    )
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
            native_capability("chip8.framebuffer", CostClass::Bounded),
            native_capability("chip8.execution_summary", CostClass::Bounded),
            native_capability("chip8.interestingness", CostClass::Heavy),
            native_capability("chip8.coverage", CostClass::Linear),
            native_capability("chip8.trajectory_identity", CostClass::Linear),
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
            normalized_capability(
                INPUT_SUMMARY,
                CostClass::Bounded,
                vec![CapabilityDependency::InputValueEvidence {
                    selectors: vec![InputValueSelector::SchemaFamily(
                        SchemaFamilyId::new("glassvm.input.digital-key")
                            .expect("static CHIP-8 input family"),
                    )],
                }],
            ),
        ],
    }
}
