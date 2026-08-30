use std::collections::BTreeMap;

use glassvm_core::{
    ArtifactEncoding, Emission, EmissionSink, ExecutionControls, ExecutionEvent, ExecutionRequest,
    FrameArtifact, FrameCapture, InputCoordinate, InputId, InputSchedule, MachineBundle,
    MachineConfiguration, ObservationRequest, RunResult, ScheduledInput, SchemaRef, SchemaVersion,
    SinkError, StructuredValue, TypedInputPayload,
};
use hexwell_plugin::HexwellPlugin;
use serde_json::Value;

const SMOKE_ARTIFACT: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[derive(Default)]
struct RecordingSink {
    events: Vec<ExecutionEvent>,
    frames: Vec<FrameArtifact>,
    started: bool,
    finished: bool,
}

impl EmissionSink for RecordingSink {
    fn emit(&mut self, emission: Emission<'_>) -> Result<(), SinkError> {
        match emission {
            Emission::RunStarted(_) => self.started = true,
            Emission::Event(event) => self.events.push(event.clone()),
            Emission::Frame(frame) => self.frames.push(frame.clone()),
            Emission::RunFinished(_result @ &RunResult { .. }) => self.finished = true,
            _ => {}
        }
        Ok(())
    }
}

fn configuration() -> MachineConfiguration {
    MachineConfiguration::new(
        SchemaRef::new("hexwell.machine_configuration", SchemaVersion::V1),
        StructuredValue::Map(BTreeMap::from([
            (
                "lattice_topology".into(),
                StructuredValue::Text("odd-r-hex-torus-v1".into()),
            ),
            ("cooling_per_frame".into(), StructuredValue::Unsigned(1)),
            ("sweeps_per_frame".into(), StructuredValue::Unsigned(2)),
            ("machine_seed".into(), StructuredValue::Unsigned(7)),
        ])),
    )
    .expect("typed Hexwell configuration")
}

fn request(bundle: &HexwellPlugin, observation: ObservationRequest) -> ExecutionRequest {
    ExecutionRequest::new(
        "hexwell-clean-contract",
        bundle.descriptor().id.clone(),
        SMOKE_ARTIFACT.to_vec(),
        configuration(),
        InputSchedule {
            schema: glassvm_core::input_schedule_schema(),
            entries: vec![ScheduledInput {
                ordinal: 0,
                coordinate: InputCoordinate::frame(0),
                input_id: InputId::new("hexwell.tide").unwrap(),
                payload: TypedInputPayload::new(
                    SchemaRef::new("hexwell.input.tide", SchemaVersion::V1),
                    StructuredValue::Unsigned(0xc1),
                )
                .unwrap(),
            }],
        },
        observation,
        ExecutionControls {
            frame_limit: Some(2),
            step_limit: None,
            bundle_limits: Vec::new(),
        },
    )
    .expect("clean execution request")
}

fn prepared_session(
    bundle: &HexwellPlugin,
    request: ExecutionRequest,
) -> Box<dyn glassvm_core::EmulatorSession> {
    let prepared_observation = bundle
        .prepare_observation(&request.observation)
        .expect("prepare observation");
    let request = request.with_prepared_observation_id(prepared_observation.identity);
    let prepared_run = bundle.prepare_run(&request).expect("prepare run");
    bundle
        .emulator()
        .create_execution_with_prepared_run(
            SMOKE_ARTIFACT,
            request,
            prepared_run,
            prepared_observation,
        )
        .expect("construct prepared session")
}

#[test]
fn contract_declares_typed_artifact_and_scheduled_tide_input() {
    let bundle = HexwellPlugin::new();
    bundle.validate_contract().expect("valid contract");
    assert_eq!(
        bundle.contract().artifact.encoding,
        ArtifactEncoding::RawBytes
    );
    assert!(bundle.contract().artifact.validate(SMOKE_ARTIFACT).is_ok());
    assert_eq!(bundle.contract().inputs.inputs.len(), 1);
    assert_eq!(
        bundle.contract().inputs.inputs[0].id.as_str(),
        "hexwell.tide"
    );
    assert!(
        bundle.contract().inputs.inputs[0]
            .delivery_modes
            .contains(&glassvm_core::InputDeliveryMode::Scheduled)
    );
    assert!(bundle.contract().inputs.inputs[0].schema_family.is_none());
}

#[test]
fn prepared_run_consumes_typed_schedule_and_emits_independent_frames() {
    let bundle = HexwellPlugin::new();
    let mut session = prepared_session(&bundle, request(&bundle, ObservationRequest::debug()));
    let mut trace = RecordingSink::default();
    let result = session
        .execute(&mut trace)
        .expect("execute prepared session");
    assert!(result.common.boot_success);
    assert_eq!(result.common.frames, 2);
    assert_eq!(trace.frames.len(), 2);
    assert_eq!(
        trace
            .events
            .iter()
            .filter(|event| event.kind == glassvm_core::EventKind::FrameCompleted)
            .count(),
        2
    );
}

#[test]
fn frame_capture_mode_is_independent_of_frame_completion_events() {
    for (capture, expected) in [
        (FrameCapture::None, 0),
        (FrameCapture::Hashes, 2),
        (FrameCapture::Full, 2),
    ] {
        let bundle = HexwellPlugin::new();
        let mut observation = ObservationRequest::summary();
        observation.frames.capture = capture;
        let mut session = prepared_session(&bundle, request(&bundle, observation));
        let mut trace = RecordingSink::default();
        session.execute(&mut trace).expect("execute capture mode");
        assert_eq!(trace.frames.len(), expected);
        assert!(
            trace
                .events
                .iter()
                .all(|event| event.kind != glassvm_core::EventKind::FrameCompleted)
        );
    }
}

#[test]
fn session_snapshot_restores_resumable_state_without_frame_history() {
    let bundle = HexwellPlugin::new();
    let mut session = prepared_session(&bundle, request(&bundle, ObservationRequest::summary()));
    let initial = session.snapshot().expect("initial snapshot");
    let encoded: Value = serde_json::from_slice(&initial).expect("decode snapshot");
    assert!(encoded.get("frame_history").is_none());
    session.step_frame().expect("step frame");
    let advanced = session.snapshot().expect("advanced snapshot");
    assert_ne!(advanced, initial);
    session.reset().expect("reset");
    session.restore_snapshot(&initial).expect("restore initial");
    assert_eq!(session.snapshot().expect("restored snapshot"), initial);
}

#[test]
fn incompatible_snapshot_version_is_rejected_before_decode() {
    let bundle = HexwellPlugin::new();
    let mut session = prepared_session(&bundle, request(&bundle, ObservationRequest::summary()));
    let snapshot = session.snapshot().expect("snapshot");
    let mut encoded: Value = serde_json::from_slice(&snapshot).expect("decode snapshot");
    encoded["snapshot_version"] = Value::from(3);
    let error = session
        .restore_snapshot(&serde_json::to_vec(&encoded).expect("encode old snapshot"))
        .expect_err("old snapshot must be rejected");
    assert!(error.contains("unsupported Hexwell snapshot version"));
}
