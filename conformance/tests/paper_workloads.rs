use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chip8_bundle::Chip8Plugin;
use glassvm_core::{
    CapabilityId, CapabilityOutput, CapabilityReceipt, CapabilityRequest, CapabilityStatus,
    Emission, EmissionSink, EventKind, EventSelection, ExecutionControls, ExecutionRequest,
    FrameCapture, FrameEvidence, InputSchedule, MachineBundle, MachineConfiguration,
    ObservationRequest, SinkError, StructuredValue,
};
use pico8_bundle::Pico8Plugin;
use tic80_bundle::Tic80Plugin;

const FRAME_LIMIT: u64 = 600;

struct Workload {
    bundle: Arc<dyn MachineBundle>,
    artifact: &'static [u8],
    configuration: MachineConfiguration,
    expected_termination: &'static str,
    visual_capability: &'static str,
    frame_metric: &'static str,
    changed_frame_metric: &'static str,
    required_events: Vec<EventKind>,
}

fn workloads() -> Vec<Workload> {
    let chip8: Arc<dyn MachineBundle> = Arc::new(Chip8Plugin::new());
    let pico8: Arc<dyn MachineBundle> = Arc::new(Pico8Plugin::new());
    let tic80: Arc<dyn MachineBundle> = Arc::new(Tic80Plugin::new());
    vec![
        Workload {
            configuration: configuration(
                chip8.as_ref(),
                BTreeMap::from([
                    ("quirks", StructuredValue::Text("chip8".into())),
                    ("cycles_per_frame", StructuredValue::Unsigned(12)),
                    ("machine_seed", StructuredValue::Unsigned(0)),
                ]),
            ),
            bundle: chip8,
            artifact: include_bytes!("../../chip8/fixtures/paper/octojam2title.ch8"),
            expected_termination: "Timeout",
            visual_capability: "chip8.visual_trajectory_motifs",
            frame_metric: "frame_artifacts",
            changed_frame_metric: "frame_hash_changes",
            required_events: vec![
                EventKind::FrameCompleted,
                EventKind::DisplayWrite,
                EventKind::InputSampled,
            ],
        },
        Workload {
            configuration: configuration(
                pico8.as_ref(),
                BTreeMap::from([
                    ("cycles_per_frame", StructuredValue::Unsigned(1)),
                    ("instruction_budget", StructuredValue::Unsigned(500_000)),
                    ("machine_seed", StructuredValue::Unsigned(0)),
                ]),
            ),
            bundle: pico8,
            artifact: include_bytes!("../../pico8/fixtures/paper/fireintro.p8"),
            expected_termination: "frame_limit",
            visual_capability: "pico8.visual.motifs",
            frame_metric: "frames",
            changed_frame_metric: "changed_frames",
            required_events: vec![EventKind::FrameCompleted],
        },
        Workload {
            configuration: configuration(
                tic80.as_ref(),
                BTreeMap::from([
                    ("cycles_per_frame", StructuredValue::Unsigned(1)),
                    ("runtime", StructuredValue::Text("lua".into())),
                    ("machine_seed", StructuredValue::Unsigned(0)),
                ]),
            ),
            bundle: tic80,
            artifact: include_bytes!("../../tic80/fixtures/paper/game.tic"),
            expected_termination: "frame_limit",
            visual_capability: "tic80.visual.motifs",
            frame_metric: "frames",
            changed_frame_metric: "changed_frames",
            required_events: vec![
                EventKind::FrameCompleted,
                EventKind::InputSampled,
                EventKind::Extension("tic80.trace".into()),
            ],
        },
    ]
}

fn configuration(
    bundle: &dyn MachineBundle,
    values: BTreeMap<&str, StructuredValue>,
) -> MachineConfiguration {
    MachineConfiguration::new(
        bundle.emulator().config_schema().schema,
        StructuredValue::Map(
            values
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        ),
    )
    .expect("typed paper workload configuration")
}

#[derive(Default)]
struct FrameEvidenceCheck {
    coordinates: Vec<(u64, u64, u64)>,
    fingerprints: BTreeSet<Vec<u8>>,
    capability_outputs: Vec<CapabilityOutput>,
    capability_receipts: Vec<CapabilityReceipt>,
}

impl EmissionSink for FrameEvidenceCheck {
    fn emit(&mut self, emission: Emission<'_>) -> Result<(), SinkError> {
        if let Emission::Frame(artifact) = emission {
            let FrameEvidence::Fingerprint { bytes } = &artifact.evidence else {
                return Err(SinkError::new(
                    "hash capture emitted non-fingerprint frame data",
                ));
            };
            self.coordinates
                .push((artifact.sequence, artifact.step, artifact.frame));
            self.fingerprints.insert(bytes.clone());
        }
        Ok(())
    }

    fn record_capability_outputs(&mut self, outputs: &[CapabilityOutput]) {
        self.capability_outputs.extend_from_slice(outputs);
    }

    fn record_capability_receipts(&mut self, receipts: &[CapabilityReceipt]) {
        self.capability_receipts.extend_from_slice(receipts);
    }
}

#[test]
fn public_zero_input_cartridges_reach_the_same_600_frame_observation_contract() {
    for workload in workloads() {
        let machine = workload.bundle.descriptor().id.clone();
        let mut observation = ObservationRequest::summary();
        observation.frames.capture = FrameCapture::Hashes;
        observation.normalized_events.events =
            EventSelection::Kinds(workload.required_events.iter().cloned().collect());
        let capability_id = CapabilityId::new(workload.visual_capability).unwrap();
        observation.capabilities = vec![CapabilityRequest::required(capability_id.clone())];
        let mut request = ExecutionRequest::new(
            format!("paper-zero-input-{machine}"),
            machine.clone(),
            workload.artifact.to_vec(),
            workload.configuration.clone(),
            InputSchedule::empty(),
            observation,
            ExecutionControls {
                frame_limit: Some(FRAME_LIMIT),
                ..ExecutionControls::default()
            },
        )
        .expect("prepared workload request");
        let prepared_observation = workload
            .bundle
            .prepare_observation(&request.observation)
            .unwrap_or_else(|error| panic!("{machine}: observation negotiation: {error:?}"));
        request = request.with_prepared_observation_id(prepared_observation.identity);
        let prepared_run = workload
            .bundle
            .prepare_run(&request)
            .unwrap_or_else(|error| panic!("{machine}: run preparation: {error}"));
        assert!(
            prepared_run.input_schedule.entries.is_empty(),
            "{machine}: workload unexpectedly has scheduled input"
        );
        let mut session = workload
            .bundle
            .emulator()
            .create_execution_with_prepared_run(
                workload.artifact,
                request,
                prepared_run,
                prepared_observation,
            )
            .unwrap_or_else(|error| panic!("{machine}: session construction: {error}"));
        let mut sink = FrameEvidenceCheck::default();
        let result = session
            .execute(&mut sink)
            .unwrap_or_else(|error| panic!("{machine}: workload execution: {error}"));

        assert!(
            result.common.boot_success,
            "{machine}: cartridge did not boot"
        );
        assert_eq!(result.common.frames, FRAME_LIMIT, "{machine}: short run");
        assert_eq!(
            result.common.termination, workload.expected_termination,
            "{machine}: unexpected termination reason"
        );
        assert_eq!(sink.coordinates.len(), FRAME_LIMIT as usize, "{machine}");
        assert!(
            sink.fingerprints.len() > 1,
            "{machine}: static/empty workload"
        );
        let output = sink
            .capability_outputs
            .iter()
            .find(|output| output.schema.id == capability_id)
            .unwrap_or_else(|| panic!("{machine}: visual capability output missing"));
        assert_eq!(
            sink.capability_outputs
                .iter()
                .filter(|output| output.schema.id == capability_id)
                .count(),
            1,
            "{machine}: expected one canonical visual output"
        );
        let receipt = sink
            .capability_receipts
            .iter()
            .find(|receipt| receipt.id == capability_id)
            .unwrap_or_else(|| panic!("{machine}: visual capability receipt missing"));
        assert_eq!(receipt.status, CapabilityStatus::Fulfilled, "{machine}");
        assert!(
            receipt.logical_bytes > 0,
            "{machine}: empty capability receipt"
        );
        assert_eq!(
            sink.capability_receipts
                .iter()
                .filter(|receipt| receipt.id == capability_id)
                .count(),
            1,
            "{machine}: expected one visual capability receipt"
        );
        assert!(
            output.value.is_object(),
            "{machine}: expected structured output"
        );
        assert_eq!(
            output.value[workload.frame_metric].as_u64(),
            Some(FRAME_LIMIT),
            "{machine}: visual reducer did not consume every frame"
        );
        assert!(
            output.value[workload.changed_frame_metric]
                .as_u64()
                .is_some_and(|count| count > 0),
            "{machine}: visual reducer did not observe changing frames"
        );
        for (index, &(_sequence, step, frame)) in sink.coordinates.iter().enumerate() {
            assert_eq!(frame, index as u64, "{machine}: frame identity");
            if index > 0 {
                assert!(
                    step > sink.coordinates[index - 1].1,
                    "{machine}: machine step did not advance with frames"
                );
            }
        }
    }
}
