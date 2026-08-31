use glassvm_core::{
    EmulatorSession, ExecutionControls, ExecutionRequest, InputSchedule, MachineBundle,
    MachineConfiguration, ObservationRequest, SchemaVersion,
};
use tic80_plugin::Tic80Plugin;

fn request(bundle: &Tic80Plugin) -> ExecutionRequest {
    ExecutionRequest::new(
        "tic80-snapshot",
        "tic80",
        include_bytes!("../../fixtures/smoke.rom").to_vec(),
        MachineConfiguration::defaults(&bundle.emulator().config_schema()).unwrap(),
        InputSchedule::empty(),
        ObservationRequest::summary(),
        ExecutionControls {
            frame_limit: Some(2),
            ..ExecutionControls::default()
        },
    )
    .unwrap()
}

fn prepared_session(bundle: &Tic80Plugin, request: ExecutionRequest) -> Box<dyn EmulatorSession> {
    let observation = bundle.prepare_observation(&request.observation).unwrap();
    let request = request.with_prepared_observation_id(observation.identity);
    let prepared = bundle.prepare_run(&request).unwrap();
    bundle
        .emulator()
        .create_execution_with_prepared_run(
            include_bytes!("../../fixtures/smoke.rom"),
            request,
            prepared,
            observation,
        )
        .unwrap()
}

#[test]
fn session_snapshot_contains_state_but_no_observation_history() {
    let bundle = Tic80Plugin::new();
    assert_eq!(
        bundle.descriptor().semantics.state_schema.version,
        SchemaVersion::new(2, 0, 0)
    );
    let mut session = prepared_session(&bundle, request(&bundle));
    session.step_frame().unwrap();
    let bytes = session.snapshot().unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("tic80.session_snapshot"));
    assert!(text.contains("machine_state"));
    assert!(!text.contains("input_history"));
    assert!(!text.contains("traces"));
}

#[test]
fn state_only_snapshot_restores_and_continues_without_history() {
    let bundle = Tic80Plugin::new();
    let mut source = prepared_session(&bundle, request(&bundle));
    source.step_frame().unwrap();
    let snapshot = source.snapshot().unwrap();

    let mut restored = prepared_session(&bundle, request(&bundle));
    restored.restore_snapshot(&snapshot).unwrap();
    restored.step_frame().unwrap();

    let mut expected = prepared_session(&bundle, request(&bundle));
    expected.step_frame().unwrap();
    expected.step_frame().unwrap();
    assert_eq!(restored.snapshot().unwrap(), expected.snapshot().unwrap());
}

#[test]
fn incompatible_snapshot_version_is_rejected_explicitly() {
    let bundle = Tic80Plugin::new();
    let mut session = prepared_session(&bundle, request(&bundle));
    session.step_frame().unwrap();
    let bytes = session.snapshot().unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["version"] = serde_json::json!(1);
    let error = session
        .restore_snapshot(&serde_json::to_vec(&value).unwrap())
        .unwrap_err();
    assert!(error.contains("unsupported TIC-80 session snapshot version"));
}
