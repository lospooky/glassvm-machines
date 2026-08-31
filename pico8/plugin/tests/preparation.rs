use std::collections::BTreeMap;

use glassvm_core::{
    Emission, EmissionSink, ExecutionControls, ExecutionRequest, InputCoordinate, InputId,
    InputSchedule, MachineBundle, MachineConfiguration, ObservationRequest, SinkError,
    StructuredValue, TypedInputPayload,
};
use pico8_plugin::Pico8Plugin;

#[derive(Default)]
struct Sink {
    started: bool,
    finished: bool,
}

impl EmissionSink for Sink {
    fn emit(&mut self, emission: Emission<'_>) -> Result<(), SinkError> {
        match emission {
            Emission::RunStarted(_) => self.started = true,
            Emission::RunFinished(_) => self.finished = true,
            _ => {}
        }
        Ok(())
    }
}

fn request(artifact: &[u8]) -> ExecutionRequest {
    let bundle = Pico8Plugin::new();
    ExecutionRequest::new(
        "pico8-preparation",
        "pico8",
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

#[test]
fn contract_and_preparation_are_clean() {
    let bundle = Pico8Plugin::new();
    bundle.validate_contract().unwrap();
    let fixture = include_bytes!("../../fixtures/smoke.rom");
    let request = request(fixture);
    let observation = bundle.prepare_observation(&request.observation).unwrap();
    let request = request.with_prepared_observation_id(observation.identity);
    let prepared = bundle.prepare_run(&request).unwrap();
    assert_eq!(prepared.machine_id.as_str(), "pico8");
    assert_eq!(prepared.input_schedule.entries.len(), 0);
    assert!(!prepared.identity.canonical_bytes().is_empty());
}

#[test]
fn invalid_artifact_fails_during_preparation() {
    let bundle = Pico8Plugin::new();
    let request = request(b"not a cartridge");
    let error = bundle.prepare_run(&request).unwrap_err();
    assert!(error.contains("invalid PICO-8 cartridge"), "{error}");
}

#[test]
fn invalid_configuration_fails_during_preparation() {
    let bundle = Pico8Plugin::new();
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
    let bundle = Pico8Plugin::new();
    let payload = TypedInputPayload::new(
        glassvm_core::SchemaRef::new("pico8.input.button", glassvm_core::SchemaVersion::V1),
        StructuredValue::Bool(true),
    )
    .unwrap();
    let mut request = request(include_bytes!("../../fixtures/smoke.rom"));
    request.input_schedule = InputSchedule {
        schema: glassvm_core::input_schedule_schema(),
        entries: vec![glassvm_core::ScheduledInput {
            ordinal: 0,
            coordinate: InputCoordinate::frame(0),
            input_id: InputId::new("pico8.unknown").unwrap(),
            payload,
        }],
    };
    let error = bundle.prepare_run(&request).unwrap_err();
    assert!(error.contains("undeclared input"), "{error}");
}

#[test]
fn prepared_execution_uses_the_negotiated_contract() {
    let bundle = Pico8Plugin::new();
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
