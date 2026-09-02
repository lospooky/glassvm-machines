use std::collections::{BTreeMap, BTreeSet};

use glassvm_core::{
    CapabilityDependency, CapabilityDescriptor, CapabilityId, CapabilityOutput, CapabilityReceipt,
    CapabilityRequest, CapabilityStatus, CostClass, Emission, EventKind, ExecutionEvent,
    InputValueSelector, Normalizer, NormalizerCatalog, NormalizerError, NormalizerOutput,
    SchemaFamilyId, SnapshotArtifact, canonical_json_bytes, standard_capabilities,
};

pub(crate) const INPUT_SUMMARY: &str = "wyrd16.input_summary";

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

    fn value(&self) -> serde_json::Value {
        serde_json::json!({
            "values": self.values,
            "distinct_inputs": self.distinct_inputs.len(),
        })
    }
}

fn capability(id: &str, output_type: &str, cost_class: CostClass) -> CapabilityDescriptor {
    CapabilityDescriptor {
        schema: CapabilityId::new(id)
            .expect("static Wyrd-16 capability ID")
            .schema(),
        output_type: output_type.into(),
        dependencies: vec![
            CapabilityDependency::NativeEvidence {
                schema_id: "wyrd16.observation".into(),
            },
            CapabilityDependency::NativeEventKinds {
                kinds: vec![id.into()],
            },
        ],
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
            .expect("static Wyrd-16 capability ID")
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

pub(crate) struct Wyrd16Normalizer {
    requests: Vec<CapabilityRequest>,
    native_outputs: BTreeMap<CapabilityId, CapabilityOutput>,
    control_flow: ControlFlowMotifState,
    state_motifs: StateMotifState,
    input_summary: InputSummaryState,
}

impl Wyrd16Normalizer {
    pub(crate) fn new() -> Self {
        Self {
            requests: Vec::new(),
            native_outputs: BTreeMap::new(),
            control_flow: ControlFlowMotifState::default(),
            state_motifs: StateMotifState::default(),
            input_summary: InputSummaryState::default(),
        }
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

    fn value(&self) -> serde_json::Value {
        serde_json::json!({
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

#[derive(Debug, Default)]
struct StateMotifState {
    read_events: u64,
    write_events: u64,
    state_diff_events: u64,
    reads: u64,
    writes: u64,
    max_reads_per_event: u64,
    max_writes_per_event: u64,
    register_writes: u64,
    memory_writes: u64,
    stack_writes: u64,
    timer_writes: u64,
    display_writes: u64,
    input_writes: u64,
    output_writes: u64,
    randomness_writes: u64,
    extension_writes: u64,
}

impl StateMotifState {
    fn observe(&mut self, event: &ExecutionEvent) {
        let read_count = event.reads.len() as u64;
        let write_count = event.writes.len() as u64;
        self.reads = self.reads.saturating_add(read_count);
        self.writes = self.writes.saturating_add(write_count);
        self.max_reads_per_event = self.max_reads_per_event.max(read_count);
        self.max_writes_per_event = self.max_writes_per_event.max(write_count);
        if read_count > 0 {
            self.read_events = self.read_events.saturating_add(1);
        }
        if write_count > 0 {
            self.write_events = self.write_events.saturating_add(1);
        }
        if event
            .writes
            .iter()
            .any(|write| write.before.is_some() && write.after.is_some())
        {
            self.state_diff_events = self.state_diff_events.saturating_add(1);
        }
        for write in &event.writes {
            let counter = match &write.location.space {
                glassvm_core::StateSpace::Register => &mut self.register_writes,
                glassvm_core::StateSpace::Memory => &mut self.memory_writes,
                glassvm_core::StateSpace::Stack => &mut self.stack_writes,
                glassvm_core::StateSpace::Timer => &mut self.timer_writes,
                glassvm_core::StateSpace::Display => &mut self.display_writes,
                glassvm_core::StateSpace::Input => &mut self.input_writes,
                glassvm_core::StateSpace::Output => &mut self.output_writes,
                glassvm_core::StateSpace::Randomness => &mut self.randomness_writes,
                glassvm_core::StateSpace::Extension(_) => &mut self.extension_writes,
            };
            *counter = counter.saturating_add(1);
        }
    }

    fn value(&self) -> serde_json::Value {
        serde_json::json!({
            "schema": "bounded_memory_state_motifs",
            "read_events": self.read_events,
            "write_events": self.write_events,
            "state_diff_events": self.state_diff_events,
            "reads": self.reads,
            "writes": self.writes,
            "max_reads_per_event": self.max_reads_per_event,
            "max_writes_per_event": self.max_writes_per_event,
            "register_writes": self.register_writes,
            "memory_writes": self.memory_writes,
            "stack_writes": self.stack_writes,
            "timer_writes": self.timer_writes,
            "display_writes": self.display_writes,
            "input_writes": self.input_writes,
            "output_writes": self.output_writes,
            "randomness_writes": self.randomness_writes,
            "extension_writes": self.extension_writes,
        })
    }
}

impl Normalizer for Wyrd16Normalizer {
    fn begin(&mut self, requests: &[CapabilityRequest]) -> Result<(), NormalizerError> {
        self.requests = requests.to_vec();
        self.native_outputs.clear();
        self.control_flow = ControlFlowMotifState::default();
        self.state_motifs = StateMotifState::default();
        self.input_summary = InputSummaryState::default();
        Ok(())
    }

    fn observe(&mut self, emission: &Emission<'_>) -> Result<(), NormalizerError> {
        match emission {
            Emission::Event(event) => {
                if self.requests.iter().any(|request| {
                    request.id.as_str() == standard_capabilities::CONTROL_FLOW_MOTIFS
                }) {
                    self.control_flow.observe(event);
                }
                if self.requests.iter().any(|request| {
                    request.id.as_str() == standard_capabilities::MEMORY_STATE_MOTIFS
                }) {
                    self.state_motifs.observe(event);
                }
            }
            Emission::NativeEvidence(evidence) => {
                let id = CapabilityId::new(&evidence.native_event.kind).map_err(|_| {
                    NormalizerError::new(format!(
                        "Wyrd-16 native evidence kind is not a canonical capability ID: {}",
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
            Emission::InputValueEvidence(evidence)
                if self
                    .requests
                    .iter()
                    .any(|request| request.id.as_str() == INPUT_SUMMARY) =>
            {
                self.input_summary.observe(evidence);
            }
            _ => {}
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
            let output = match request.id.as_str() {
                standard_capabilities::CONTROL_FLOW_MOTIFS => Some(CapabilityOutput {
                    schema: request.id.schema(),
                    value: self.control_flow.value(),
                }),
                standard_capabilities::MEMORY_STATE_MOTIFS => Some(CapabilityOutput {
                    schema: request.id.schema(),
                    value: self.state_motifs.value(),
                }),
                INPUT_SUMMARY => Some(CapabilityOutput {
                    schema: request.id.schema(),
                    value: self.input_summary.value(),
                }),
                _ => self.native_outputs.get(&request.id).cloned(),
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
                    message: Some("requested evidence produced no capability output".into()),
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

pub fn catalog() -> NormalizerCatalog {
    NormalizerCatalog {
        schema_version: glassvm_core::NORMALIZER_CONTRACT_SCHEMA_VERSION,
        normalizer_id: "wyrd16.normalizer".into(),
        normalizer_version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            capability("wyrd16.canvas", "json.object", CostClass::Bounded),
            capability("wyrd16.palette", "json.object", CostClass::Negligible),
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
                standard_capabilities::MEMORY_STATE_MOTIFS,
                CostClass::Bounded,
                vec![CapabilityDependency::NormalizedState {
                    reads: true,
                    writes: true,
                    state_diffs: true,
                    access: glassvm_core::NormalizedAccessRequirement::BeforeAndAfter,
                }],
            ),
            normalized_capability(
                INPUT_SUMMARY,
                CostClass::Bounded,
                vec![CapabilityDependency::InputValueEvidence {
                    selectors: vec![InputValueSelector::SchemaFamily(
                        SchemaFamilyId::new("glassvm.input.digital-key")
                            .expect("static Wyrd-16 input family"),
                    )],
                }],
            ),
        ],
    }
}
