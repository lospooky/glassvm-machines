use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chip8_plugin::Chip8Plugin;
use glassvm_core::{
    AccessDetail, CapabilityDependency, CapabilityId, CapabilityOutput, CapabilityReceipt,
    CapabilityRequest, CapabilityStatus, CostClass, Emission, EmissionSink, EventSelection,
    ExecutionControls, ExecutionRequest, FrameCapture, InputCoordinate, InputId, InputSchedule,
    MachineBundle, MachineConfiguration, ObservationRequest, ScheduledInput, SchemaRef,
    SchemaVersion, SinkError, SnapshotCapture, StructuredValue, TypedInputPayload,
    canonical_json_bytes,
};
use hexwell_plugin::HexwellPlugin;
use pico8_plugin::Pico8Plugin;
use tic80_plugin::Tic80Plugin;
use wyrd16_plugin::Wyrd16Plugin;

const MAX_BOUNDED_SUMMARY_BYTES: usize = 4096;

struct BundleCase {
    bundle: Arc<dyn MachineBundle>,
    fixture: &'static [u8],
    native: &'static str,
    semantic: &'static str,
    behavior: &'static str,
    behavior_activity_field: &'static str,
    input: &'static str,
    input_id: &'static str,
    input_schema: &'static str,
    input_value: StructuredValue,
}

fn cases() -> Vec<BundleCase> {
    vec![
        BundleCase {
            bundle: Arc::new(Chip8Plugin::new()),
            fixture: include_bytes!("../../chip8/fixtures/smoke.rom"),
            native: "chip8.execution_summary",
            semantic: glassvm_core::standard_capabilities::CONTROL_FLOW_MOTIFS,
            behavior: "chip8.visual_trajectory_motifs",
            behavior_activity_field: "frame_artifacts",
            input: "chip8.input_summary",
            input_id: "chip8.key.0",
            input_schema: "chip8.input.key",
            input_value: StructuredValue::Bool(true),
        },
        BundleCase {
            bundle: Arc::new(HexwellPlugin::new()),
            fixture: include_bytes!("../../hexwell/fixtures/smoke.rom"),
            native: "hexwell.reaction_dynamics",
            semantic: glassvm_core::standard_capabilities::CONTROL_FLOW_MOTIFS,
            behavior: "hexwell.reaction_field_motifs",
            behavior_activity_field: "sweep_events",
            input: "hexwell.input_summary",
            input_id: "hexwell.tide",
            input_schema: "hexwell.input.tide",
            input_value: StructuredValue::Unsigned(0x41),
        },
        BundleCase {
            bundle: Arc::new(Wyrd16Plugin::new()),
            fixture: include_bytes!("../../wyrd16/fixtures/smoke.rom"),
            native: "wyrd16.canvas",
            semantic: glassvm_core::standard_capabilities::CONTROL_FLOW_MOTIFS,
            behavior: glassvm_core::standard_capabilities::MEMORY_STATE_MOTIFS,
            behavior_activity_field: "writes",
            input: "wyrd16.input_summary",
            input_id: "wyrd16.key.0",
            input_schema: "wyrd16.input.key",
            input_value: StructuredValue::Bool(true),
        },
        BundleCase {
            bundle: Arc::new(Pico8Plugin::new()),
            fixture: include_bytes!("../../pico8/fixtures/smoke.rom"),
            native: "pico8.execution_summary",
            semantic: "pico8.visual.motifs",
            behavior: "pico8.visual.motifs",
            behavior_activity_field: "frames",
            input: "pico8.input_summary",
            input_id: "pico8.button.0",
            input_schema: "pico8.input.button",
            input_value: StructuredValue::Bool(true),
        },
        BundleCase {
            bundle: Arc::new(Tic80Plugin::new()),
            fixture: include_bytes!("../../tic80/fixtures/smoke.rom"),
            native: "tic80.execution_summary",
            semantic: "tic80.visual.motifs",
            behavior: "tic80.visual.motifs",
            behavior_activity_field: "frames",
            input: "tic80.input_summary",
            input_id: "tic80.gamepad",
            input_schema: "tic80.input.gamepad",
            input_value: StructuredValue::Unsigned(1),
        },
    ]
}

fn one_input(case: &BundleCase) -> InputSchedule {
    InputSchedule {
        schema: glassvm_core::input_schedule_schema(),
        entries: vec![ScheduledInput {
            ordinal: 0,
            coordinate: InputCoordinate::frame(0),
            input_id: InputId::new(case.input_id).unwrap(),
            payload: TypedInputPayload::new(
                SchemaRef::new(case.input_schema, SchemaVersion::V1),
                case.input_value.clone(),
            )
            .unwrap(),
        }],
    }
}

fn complete_observation(
    capabilities: impl IntoIterator<Item = CapabilityId>,
    frame_capture: FrameCapture,
) -> ObservationRequest {
    let mut observation = ObservationRequest::summary();
    observation.normalized_events.events = EventSelection::All;
    observation.normalized_events.access_detail = AccessDetail::BeforeAndAfter;
    observation.normalized_events.state_diffs = true;
    observation.native_evidence.enabled = true;
    observation.native_evidence.all = true;
    observation.frames.capture = frame_capture;
    observation.snapshots.capture = SnapshotCapture::Final;
    observation.input_value_evidence.enabled = true;
    observation.capabilities = capabilities
        .into_iter()
        .map(CapabilityRequest::required)
        .collect();
    observation
}

#[derive(Default)]
struct CapabilitySink {
    outputs: Vec<CapabilityOutput>,
    receipts: Vec<CapabilityReceipt>,
    result_outputs: Vec<CapabilityOutput>,
}

impl EmissionSink for CapabilitySink {
    fn emit(&mut self, emission: Emission<'_>) -> Result<(), SinkError> {
        if let Emission::RunFinished(result) = emission {
            self.result_outputs = result.capabilities.clone();
        }
        Ok(())
    }

    fn record_capability_outputs(&mut self, outputs: &[CapabilityOutput]) {
        self.outputs.extend_from_slice(outputs);
    }

    fn record_capability_receipts(&mut self, receipts: &[CapabilityReceipt]) {
        self.receipts.extend_from_slice(receipts);
    }
}

fn execute(
    case: &BundleCase,
    capability_ids: Vec<CapabilityId>,
    frame_limit: u64,
) -> CapabilitySink {
    execute_with_capture(case, capability_ids, frame_limit, FrameCapture::Full)
}

fn execute_with_capture(
    case: &BundleCase,
    capability_ids: Vec<CapabilityId>,
    frame_limit: u64,
    frame_capture: FrameCapture,
) -> CapabilitySink {
    let observation = complete_observation(capability_ids, frame_capture);
    let mut request = ExecutionRequest::new(
        format!("{}-derived-capabilities", case.bundle.descriptor().id),
        case.bundle.descriptor().id.clone(),
        case.fixture.to_vec(),
        MachineConfiguration::defaults(&case.bundle.emulator().config_schema()).unwrap(),
        one_input(case),
        observation,
        ExecutionControls {
            frame_limit: Some(frame_limit),
            step_limit: None,
            bundle_limits: Vec::new(),
        },
    )
    .unwrap();
    let prepared_observation = case
        .bundle
        .prepare_observation(&request.observation)
        .unwrap_or_else(|errors| panic!("{}: {}", request.machine_id, errors.join("; ")));
    request = request.with_prepared_observation_id(prepared_observation.identity);
    let prepared_run = case.bundle.prepare_run(&request).unwrap();
    let mut session = case
        .bundle
        .emulator()
        .create_execution_with_prepared_run(
            case.fixture,
            request,
            prepared_run,
            prepared_observation,
        )
        .unwrap();
    let mut sink = CapabilitySink::default();
    session.execute(&mut sink).unwrap();
    sink
}

fn output_map(outputs: &[CapabilityOutput]) -> BTreeMap<CapabilityId, &CapabilityOutput> {
    outputs
        .iter()
        .map(|output| (output.schema.id.clone(), output))
        .collect()
}

#[test]
fn every_published_capability_produces_one_canonical_output_and_receipt() {
    for case in cases() {
        let catalog = case.bundle.normalizer_catalog();
        let expected_ids = catalog
            .capabilities
            .iter()
            .map(|descriptor| descriptor.schema.id.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(expected_ids.len(), catalog.capabilities.len());

        let sink = execute(&case, expected_ids.iter().cloned().collect(), 2);
        let outputs = output_map(&sink.outputs);
        let result_outputs = output_map(&sink.result_outputs);
        let receipts = sink
            .receipts
            .iter()
            .map(|receipt| (receipt.id.clone(), receipt))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            outputs.keys().cloned().collect::<BTreeSet<_>>(),
            expected_ids
        );
        assert_eq!(
            result_outputs.keys().cloned().collect::<BTreeSet<_>>(),
            expected_ids
        );
        assert_eq!(
            receipts.keys().cloned().collect::<BTreeSet<_>>(),
            expected_ids
        );
        assert_eq!(sink.outputs.len(), expected_ids.len());
        assert_eq!(sink.receipts.len(), expected_ids.len());

        for descriptor in &catalog.capabilities {
            let id = &descriptor.schema.id;
            let output = outputs[id];
            let receipt = receipts[id];
            assert_eq!(
                output.schema,
                descriptor.schema,
                "{}: {}",
                case.bundle.descriptor().id,
                id.as_str()
            );
            assert!(
                output.value.is_object(),
                "{}: {}",
                case.bundle.descriptor().id,
                id.as_str()
            );
            assert_eq!(
                receipt.status,
                CapabilityStatus::Fulfilled,
                "{}: {}",
                case.bundle.descriptor().id,
                id.as_str()
            );
            assert_eq!(receipt.output_schema.as_ref(), Some(&descriptor.schema));
            assert_eq!(
                receipt.logical_bytes,
                canonical_json_bytes(&output.value).unwrap().len() as u64,
                "{}: {}",
                case.bundle.descriptor().id,
                id.as_str()
            );
        }

        for representative in [case.native, case.semantic, case.behavior, case.input] {
            assert!(
                outputs.contains_key(&CapabilityId::new(representative).unwrap()),
                "{} lacks representative capability {representative}",
                case.bundle.descriptor().id
            );
        }
        assert!(
            outputs[&CapabilityId::new(case.behavior).unwrap()].value[case.behavior_activity_field]
                .as_u64()
                .is_some_and(|count| count > 0),
            "{} behavior reducer observed no representative activity",
            case.bundle.descriptor().id
        );
        assert_eq!(
            outputs[&CapabilityId::new(case.input).unwrap()].value["values"].as_u64(),
            Some(1),
            "{} input reducer did not consume the selected value",
            case.bundle.descriptor().id
        );
    }
}

#[test]
fn catalogs_declare_exact_bounded_reducer_prerequisites() {
    for case in cases() {
        let catalog = case.bundle.normalizer_catalog();
        for descriptor in &catalog.capabilities {
            assert!(
                !descriptor.dependencies.is_empty(),
                "{}: {} has no evidence prerequisite",
                case.bundle.descriptor().id,
                descriptor.schema.id.as_str()
            );
            if descriptor
                .dependencies
                .iter()
                .any(|dependency| matches!(dependency, CapabilityDependency::NativeEvidence { .. }))
            {
                assert!(descriptor.dependencies.iter().any(|dependency| {
                    matches!(dependency, CapabilityDependency::NativeEventKinds { kinds }
                        if kinds == &[descriptor.schema.id.as_str().to_owned()])
                }));
            }
        }

        for id in [case.semantic, case.behavior, case.input] {
            let descriptor = catalog.find(&CapabilityId::new(id).unwrap()).unwrap();
            assert_eq!(
                descriptor.cost_class,
                CostClass::Bounded,
                "{}: {id}",
                case.bundle.descriptor().id
            );
        }
    }
}

#[test]
fn online_reducer_outputs_remain_fixed_shape_over_longer_runs() {
    for case in cases() {
        let ids = [case.semantic, case.behavior, case.input]
            .into_iter()
            .map(|id| CapabilityId::new(id).unwrap())
            .collect::<BTreeSet<_>>();
        let short = output_map(&execute(&case, ids.iter().cloned().collect(), 2).outputs)
            .into_iter()
            .map(|(id, output)| (id, output.value.clone()))
            .collect::<BTreeMap<_, _>>();
        let long = output_map(&execute(&case, ids.iter().cloned().collect(), 64).outputs)
            .into_iter()
            .map(|(id, output)| (id, output.value.clone()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            short.keys().collect::<Vec<_>>(),
            long.keys().collect::<Vec<_>>()
        );
        for id in ids {
            let short_object = short[&id].as_object().unwrap();
            let long_object = long[&id].as_object().unwrap();
            assert_eq!(
                short_object.keys().collect::<Vec<_>>(),
                long_object.keys().collect::<Vec<_>>(),
                "{}: {} changed output shape with run length",
                case.bundle.descriptor().id,
                id.as_str()
            );
            assert!(
                canonical_json_bytes(&long[&id]).unwrap().len() <= MAX_BOUNDED_SUMMARY_BYTES,
                "{}: {} exceeded the bounded summary envelope",
                case.bundle.descriptor().id,
                id.as_str()
            );
            assert!(
                long_object.values().all(|value| !value.is_array()),
                "{}: {} exposed a growing collection",
                case.bundle.descriptor().id,
                id.as_str()
            );
        }
    }
}

#[test]
fn visual_summaries_are_invariant_between_hash_and_full_frame_capture() {
    for case in cases() {
        let id = CapabilityId::new(case.behavior).unwrap();
        let hashes_sink = execute_with_capture(&case, vec![id.clone()], 2, FrameCapture::Hashes);
        let full_sink = execute_with_capture(&case, vec![id.clone()], 2, FrameCapture::Full);
        let hashes = output_map(&hashes_sink.outputs);
        let full = output_map(&full_sink.outputs);
        let mut hashes_value = hashes[&id].value.clone();
        let mut full_value = full[&id].value.clone();
        hashes_value
            .as_object_mut()
            .unwrap()
            .remove("full_artifact_count");
        full_value
            .as_object_mut()
            .unwrap()
            .remove("full_artifact_count");
        assert_eq!(
            hashes_value,
            full_value,
            "{}: visual summary changed with evidence richness",
            case.bundle.descriptor().id
        );
    }
}
