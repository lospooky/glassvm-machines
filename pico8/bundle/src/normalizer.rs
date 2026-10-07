use glassvm_core::{
    CapabilityDependency, CapabilityDescriptor, CapabilityId, CapabilityOutput, CapabilityReceipt,
    CapabilityRequest, CapabilityStatus, CostClass, Emission, EventKind, FrameCaptureRequirement,
    FrameEvidence, Normalizer, NormalizerCatalog, NormalizerError, NormalizerOutput,
    SnapshotArtifact, canonical_json_bytes,
};
use glassvm_core::{InputValueSelector, SchemaFamilyId};
use serde_json::{Value, json};

pub(crate) const VISUAL_MOTIFS: &str = "pico8.visual.motifs";
pub(crate) const EXECUTION_SUMMARY: &str = "pico8.execution_summary";
pub(crate) const INPUT_SUMMARY: &str = "pico8.input_summary";

const NATIVE_EVENT_SCHEMA: &str = "pico8.native_event";
const FRAME_COMPLETED_NATIVE: &str = "pico8.frame_completed";

#[derive(Debug, Default)]
struct VisualMotifState {
    frames: u64,
    changed_frames: u64,
    repeated_frames: u64,
    last_frame_digest: Option<[u8; 32]>,
}

#[derive(Debug, Default)]
struct InputSummaryState {
    values: u64,
    distinct_inputs: std::collections::BTreeSet<String>,
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

impl VisualMotifState {
    fn observe_event(&mut self, event: &glassvm_core::ExecutionEvent) {
        if event.kind == EventKind::FrameCompleted {
            self.frames = self.frames.saturating_add(1);
        }
    }

    fn observe_frame(&mut self, frame: &glassvm_core::FrameArtifact) {
        let digest = match &frame.evidence {
            FrameEvidence::Fingerprint { bytes } if bytes.len() == 32 => {
                let mut digest = [0; 32];
                digest.copy_from_slice(bytes);
                digest
            }
            FrameEvidence::Fingerprint { bytes } => {
                glassvm_core::ContentDigest::sha256(bytes).value
            }
            FrameEvidence::Full { bytes } => glassvm_core::ContentDigest::sha256(bytes).value,
        };
        if let Some(previous) = self.last_frame_digest {
            if previous == digest {
                self.repeated_frames = self.repeated_frames.saturating_add(1);
            } else {
                self.changed_frames = self.changed_frames.saturating_add(1);
            }
        }
        self.last_frame_digest = Some(digest);
    }

    fn value(&self) -> Value {
        json!({
            "frames": self.frames,
            "changed_frames": self.changed_frames,
            "repeated_frames": self.repeated_frames,
        })
    }
}

pub(crate) struct Pico8Normalizer {
    requests: Vec<CapabilityRequest>,
    native_outputs: std::collections::BTreeMap<CapabilityId, CapabilityOutput>,
    visual: VisualMotifState,
    input_summary: InputSummaryState,
}

impl Pico8Normalizer {
    pub(crate) fn new() -> Self {
        Self {
            requests: Vec::new(),
            native_outputs: std::collections::BTreeMap::new(),
            visual: VisualMotifState::default(),
            input_summary: InputSummaryState::default(),
        }
    }
}

impl Normalizer for Pico8Normalizer {
    fn begin(&mut self, requests: &[CapabilityRequest]) -> Result<(), NormalizerError> {
        self.requests = requests.to_vec();
        self.native_outputs.clear();
        self.visual = VisualMotifState::default();
        self.input_summary = InputSummaryState::default();
        Ok(())
    }

    fn observe(&mut self, emission: &Emission<'_>) -> Result<(), NormalizerError> {
        match emission {
            Emission::Event(event)
                if self
                    .requests
                    .iter()
                    .any(|request| request.id.as_str() == VISUAL_MOTIFS) =>
            {
                self.visual.observe_event(event);
            }
            Emission::Frame(frame)
                if self
                    .requests
                    .iter()
                    .any(|request| request.id.as_str() == VISUAL_MOTIFS) =>
            {
                self.visual.observe_frame(frame);
            }
            Emission::NativeEvidence(evidence) => {
                let id = CapabilityId::new(evidence.native_event.kind.clone())
                    .map_err(NormalizerError::new)?;
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
        _final_state: Option<&SnapshotArtifact>,
    ) -> Result<NormalizerOutput, NormalizerError> {
        let mut capabilities = Vec::new();
        let mut receipts = Vec::new();
        for request in &self.requests {
            let output = if request.id.as_str() == VISUAL_MOTIFS {
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
                    message: Some("requested capability produced no output".into()),
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

fn descriptor(
    id: &str,
    dependencies: Vec<CapabilityDependency>,
    cost_class: CostClass,
) -> CapabilityDescriptor {
    CapabilityDescriptor {
        schema: CapabilityId::new(id)
            .expect("static PICO-8 capability ID")
            .schema(),
        output_type: "json.object".into(),
        dependencies,
        cost_class,
    }
}

pub(crate) fn catalog() -> NormalizerCatalog {
    NormalizerCatalog {
        schema_version: glassvm_core::NORMALIZER_CONTRACT_SCHEMA_VERSION,
        normalizer_id: "pico8.normalizer".into(),
        normalizer_version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            descriptor(
                EXECUTION_SUMMARY,
                vec![
                    CapabilityDependency::NativeEvidence {
                        schema_id: NATIVE_EVENT_SCHEMA.into(),
                    },
                    CapabilityDependency::NativeEventKinds {
                        kinds: vec![EXECUTION_SUMMARY.into()],
                    },
                ],
                CostClass::Bounded,
            ),
            descriptor(
                VISUAL_MOTIFS,
                vec![
                    CapabilityDependency::NormalizedEvents,
                    CapabilityDependency::NormalizedEventKinds {
                        kinds: vec!["frame_completed".into()],
                    },
                    CapabilityDependency::Frames {
                        capture: FrameCaptureRequirement::Hashes,
                    },
                ],
                CostClass::Bounded,
            ),
            descriptor(
                INPUT_SUMMARY,
                vec![CapabilityDependency::InputValueEvidence {
                    selectors: vec![InputValueSelector::SchemaFamily(
                        SchemaFamilyId::new("glassvm.input.digital-key")
                            .expect("static PICO-8 input family"),
                    )],
                }],
                CostClass::Bounded,
            ),
        ],
    }
}

#[allow(dead_code)]
const _: &str = FRAME_COMPLETED_NATIVE;
