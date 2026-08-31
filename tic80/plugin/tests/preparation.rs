use std::collections::BTreeMap;

use glassvm_core::{
    CapabilityId, CapabilityRequest, Emission, EmissionSink, EventKind, EventSelection,
    ExecutionControls, ExecutionRequest, FrameCapture, FrameEvidence, InputCoordinate, InputId,
    InputSchedule, MachineBundle, MachineConfiguration, ObservationRequest, SinkError,
    StructuredValue, TypedInputPayload,
};
use tic80_plugin::Tic80Plugin;

#[derive(Default)]
struct Sink {
    started: bool,
    finished: bool,
    events: usize,
    native: usize,
    frames: usize,
    input_values: usize,
    full_frame_bytes: Vec<usize>,
    capability_ids: Vec<String>,
}

impl EmissionSink for Sink {
    fn emit(&mut self, emission: Emission<'_>) -> Result<(), SinkError> {
        match emission {
            Emission::RunStarted(_) => self.started = true,
            Emission::RunFinished(_) => self.finished = true,
            Emission::Event(_) => self.events += 1,
            Emission::NativeEvidence(_) => self.native += 1,
            Emission::Frame(frame) => {
                self.frames += 1;
                if let FrameEvidence::Full { bytes } = &frame.evidence {
                    self.full_frame_bytes.push(bytes.len());
                }
            }
            Emission::InputValueEvidence(_) => self.input_values += 1,
            _ => {}
        }
        Ok(())
    }

    fn record_capability_outputs(&mut self, outputs: &[glassvm_core::CapabilityOutput]) {
        self.capability_ids.extend(
            outputs
                .iter()
                .map(|output| output.schema.id.as_str().to_owned()),
        );
    }
}

fn request(artifact: &[u8]) -> ExecutionRequest {
    let bundle = Tic80Plugin::new();
    ExecutionRequest::new(
        "tic80-preparation",
        "tic80",
        artifact.to_vec(),
        MachineConfiguration::defaults(&bundle.emulator().config_schema()).unwrap(),
        InputSchedule::empty(),
        ObservationRequest::summary(),
        ExecutionControls {
            frame_limit: Some(1),
            ..ExecutionControls::default()
        },
    )
    .unwrap()
}

fn execute_with_observation(observation: ObservationRequest) -> Sink {
    execute_with_observation_and_schedule(observation, InputSchedule::empty())
}

fn execute_with_observation_and_schedule(
    observation: ObservationRequest,
    input_schedule: InputSchedule,
) -> Sink {
    let bundle = Tic80Plugin::new();
    let fixture = include_bytes!("../../fixtures/smoke.rom");
    let mut request = ExecutionRequest::new(
        "tic80-emission",
        "tic80",
        fixture.to_vec(),
        MachineConfiguration::defaults(&bundle.emulator().config_schema()).unwrap(),
        input_schedule,
        observation,
        ExecutionControls {
            frame_limit: Some(2),
            ..ExecutionControls::default()
        },
    )
    .unwrap();
    let prepared_observation = bundle.prepare_observation(&request.observation).unwrap();
    request = request.with_prepared_observation_id(prepared_observation.identity);
    let prepared = bundle.prepare_run(&request).unwrap();
    let mut session = bundle
        .emulator()
        .create_execution_with_prepared_run(fixture, request, prepared, prepared_observation)
        .unwrap();
    let mut sink = Sink::default();
    session.execute(&mut sink).unwrap();
    sink
}

#[test]
fn contract_and_preparation_are_clean() {
    let bundle = Tic80Plugin::new();
    bundle.validate_contract().unwrap();
    let fixture = include_bytes!("../../fixtures/smoke.rom");
    let request = request(fixture);
    let observation = bundle.prepare_observation(&request.observation).unwrap();
    let request = request.with_prepared_observation_id(observation.identity);
    let prepared = bundle.prepare_run(&request).unwrap();
    assert_eq!(prepared.machine_id.as_str(), "tic80");
    assert_eq!(prepared.input_schedule.entries.len(), 0);
    assert!(!prepared.identity.canonical_bytes().is_empty());
}

#[test]
fn invalid_artifact_fails_during_preparation() {
    let bundle = Tic80Plugin::new();
    let request = request(b"not a cartridge");
    let error = bundle.prepare_run(&request).unwrap_err();
    assert!(error.contains("invalid TIC-80 cartridge"), "{error}");
}

#[test]
fn invalid_configuration_fails_during_preparation() {
    let bundle = Tic80Plugin::new();
    let schema = bundle.emulator().config_schema().schema;
    let configuration = MachineConfiguration::new(
        schema,
        StructuredValue::Map(BTreeMap::from([(
            "unknown".into(),
            StructuredValue::Unsigned(1),
        )])),
    )
    .unwrap();
    let mut request = request(include_bytes!("../../fixtures/smoke.rom"));
    request.configuration = configuration;
    let error = bundle.prepare_run(&request).unwrap_err();
    assert!(error.contains("unknown field"), "{error}");
}

#[test]
fn undeclared_input_fails_during_preparation() {
    let bundle = Tic80Plugin::new();
    let payload = TypedInputPayload::new(
        glassvm_core::SchemaRef::new("tic80.input.gamepad", glassvm_core::SchemaVersion::V1),
        StructuredValue::Unsigned(1),
    )
    .unwrap();
    let mut request = request(include_bytes!("../../fixtures/smoke.rom"));
    request.input_schedule = InputSchedule {
        schema: glassvm_core::input_schedule_schema(),
        entries: vec![glassvm_core::ScheduledInput {
            ordinal: 0,
            coordinate: InputCoordinate::frame(0),
            input_id: InputId::new("tic80.unknown").unwrap(),
            payload,
        }],
    };
    let error = bundle.prepare_run(&request).unwrap_err();
    assert!(error.contains("undeclared input"), "{error}");
}

#[test]
fn prepared_execution_uses_the_negotiated_contract() {
    let bundle = Tic80Plugin::new();
    let fixture = include_bytes!("../../fixtures/smoke.rom");
    let mut request = request(fixture);
    let observation = bundle.prepare_observation(&request.observation).unwrap();
    request = request.with_prepared_observation_id(observation.identity);
    let prepared = bundle.prepare_run(&request).unwrap();
    let mut session = bundle
        .emulator()
        .create_execution_with_prepared_run(fixture, request, prepared, observation)
        .unwrap();
    let mut sink = Sink::default();
    let result = session.execute(&mut sink).unwrap();
    assert!(result.common.boot_success);
    assert!(sink.started);
    assert!(sink.finished);
}

#[test]
fn frame_artifacts_are_independent_of_frame_events() {
    let mut observation = ObservationRequest::summary();
    observation.frames.capture = FrameCapture::Hashes;
    let sink = execute_with_observation(observation);
    assert_eq!(sink.events, 0);
    assert_eq!(sink.frames, 2);
}

#[test]
fn full_frame_capture_uses_the_typed_full_representation() {
    let mut observation = ObservationRequest::summary();
    observation.frames.capture = FrameCapture::Full;
    let sink = execute_with_observation(observation);
    assert_eq!(sink.full_frame_bytes, vec![240 * 136 * 4; 2]);
}

#[test]
fn normalized_visual_capability_receives_frame_input_and_trace_events() {
    let mut observation = ObservationRequest::summary();
    observation.normalized_events.events = EventSelection::Kinds(
        [
            EventKind::FrameCompleted,
            EventKind::InputSampled,
            EventKind::Extension("tic80.trace".into()),
        ]
        .into_iter()
        .collect(),
    );
    observation.frames.capture = FrameCapture::Hashes;
    observation.capabilities = vec![CapabilityRequest::required(
        CapabilityId::new("tic80.visual.motifs").unwrap(),
    )];
    let sink = execute_with_observation(observation);
    assert!(sink.events >= 2);
    assert_eq!(sink.frames, 2);
    assert_eq!(sink.capability_ids, vec!["tic80.visual.motifs"]);
}

#[test]
fn native_trace_summary_requires_and_produces_native_evidence() {
    let mut observation = ObservationRequest::summary();
    observation.native_evidence.enabled = true;
    observation.native_evidence.all = true;
    observation.capabilities = vec![CapabilityRequest::required(
        CapabilityId::new("tic80.execution_summary").unwrap(),
    )];
    let sink = execute_with_observation(observation);
    assert!(sink.native >= 1);
    assert_eq!(sink.capability_ids, vec!["tic80.execution_summary"]);
}

#[test]
fn input_value_capability_receives_the_selected_gamepad_value() {
    let mut observation = ObservationRequest::summary();
    observation.input_value_evidence.enabled = true;
    observation.capabilities = vec![CapabilityRequest::required(
        CapabilityId::new("tic80.input_summary").unwrap(),
    )];
    let payload = TypedInputPayload::new(
        glassvm_core::SchemaRef::new("tic80.input.gamepad", glassvm_core::SchemaVersion::V1),
        StructuredValue::Unsigned(1),
    )
    .unwrap();
    let schedule = InputSchedule {
        schema: glassvm_core::input_schedule_schema(),
        entries: vec![glassvm_core::ScheduledInput {
            ordinal: 0,
            coordinate: InputCoordinate::frame(0),
            input_id: InputId::new("tic80.gamepad").unwrap(),
            payload,
        }],
    };
    let sink = execute_with_observation_and_schedule(observation, schedule);
    assert_eq!(sink.input_values, 1);
    assert_eq!(sink.capability_ids, vec!["tic80.input_summary"]);
}
