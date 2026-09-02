use std::collections::BTreeMap;

use chip8_plugin::Chip8Plugin;
use glassvm_core::{
    ArtifactEncoding, CapabilityOutput, CapabilityReceipt, CapabilityStatus, Emission,
    EmissionSink, ExecutionControls, ExecutionEvent, ExecutionRequest, FrameArtifact, FrameCapture,
    InputCoordinate, InputId, InputSchedule, MachineBundle, MachineConfiguration,
    ObservationRequest, RunResult, ScheduledInput, SchemaRef, SchemaVersion, SinkError,
    StructuredValue, TypedInputPayload,
};
use serde_json::Value;

const SMOKE_ROM: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[derive(Default)]
struct RecordingSink {
    events: Vec<ExecutionEvent>,
    frames: Vec<FrameArtifact>,
    started: bool,
    finished: bool,
    capability_outputs: Vec<CapabilityOutput>,
    capability_receipts: Vec<CapabilityReceipt>,
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

    fn record_capability_outputs(&mut self, outputs: &[CapabilityOutput]) {
        self.capability_outputs.extend_from_slice(outputs);
    }

    fn record_capability_receipts(&mut self, receipts: &[CapabilityReceipt]) {
        self.capability_receipts.extend_from_slice(receipts);
    }
}

fn configuration() -> MachineConfiguration {
    MachineConfiguration::new(
        SchemaRef::new("chip8.machine_configuration", SchemaVersion::new(2, 0, 0)),
        StructuredValue::Map(BTreeMap::from([
            ("quirks".into(), StructuredValue::Text("chip8".into())),
            ("cycles_per_frame".into(), StructuredValue::Unsigned(1)),
            ("machine_seed".into(), StructuredValue::Unsigned(7)),
        ])),
    )
    .expect("typed CHIP-8 configuration")
}

fn request(bundle: &Chip8Plugin, observation: ObservationRequest) -> ExecutionRequest {
    ExecutionRequest::new(
        "chip8-clean-contract",
        bundle.descriptor().id.clone(),
        SMOKE_ROM.to_vec(),
        configuration(),
        InputSchedule {
            schema: glassvm_core::input_schedule_schema(),
            entries: vec![ScheduledInput {
                ordinal: 0,
                coordinate: InputCoordinate::frame(0),
                input_id: InputId::new("chip8.key.5").unwrap(),
                payload: TypedInputPayload::new(
                    SchemaRef::new("chip8.input.key", SchemaVersion::V1),
                    StructuredValue::Bool(true),
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
    bundle: &Chip8Plugin,
    request: ExecutionRequest,
) -> Box<dyn glassvm_core::EmulatorSession> {
    let prepared_observation = bundle
        .prepare_observation(&request.observation)
        .expect("prepare observation");
    let request = request.with_prepared_observation_id(prepared_observation.identity);
    let prepared_run = bundle.prepare_run(&request).expect("prepare run");
    bundle
        .emulator()
        .create_execution_with_prepared_run(SMOKE_ROM, request, prepared_run, prepared_observation)
        .expect("construct prepared session")
}

#[test]
fn contract_declares_typed_artifact_and_per_key_inputs() {
    let bundle = Chip8Plugin::new();
    bundle.validate_contract().expect("valid contract");
    assert_eq!(
        bundle.contract().artifact.encoding,
        ArtifactEncoding::RawBytes
    );
    assert!(bundle.contract().artifact.validate(SMOKE_ROM).is_ok());
    assert_eq!(bundle.contract().inputs.inputs.len(), 16);
    assert_eq!(
        bundle.contract().inputs.inputs[0].id.as_str(),
        "chip8.key.0"
    );
    assert_eq!(
        bundle.contract().inputs.inputs[15].id.as_str(),
        "chip8.key.f"
    );
}

#[test]
fn prepared_run_consumes_typed_schedule_and_emits_independent_frames() {
    let bundle = Chip8Plugin::new();
    let mut session = prepared_session(&bundle, request(&bundle, ObservationRequest::debug()));
    let mut trace = RecordingSink::default();
    let result = session
        .execute(&mut trace)
        .expect("execute prepared session");
    assert!(result.common.boot_success);
    assert_eq!(result.common.frames, 2);
    assert_eq!(trace.frames.len(), 2);
    assert!(
        trace
            .events
            .iter()
            .any(|event| event.kind == glassvm_core::EventKind::FrameCompleted)
    );
}

#[test]
fn frame_capture_mode_is_independent_of_frame_completion_events() {
    for (capture, expected) in [
        (FrameCapture::None, 0),
        (FrameCapture::Hashes, 2),
        (FrameCapture::Full, 2),
    ] {
        let bundle = Chip8Plugin::new();
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
fn capability_outputs_and_receipts_survive_the_chip8_sink_wrapper() {
    let bundle = Chip8Plugin::new();
    let capability = glassvm_core::CapabilityId::new("chip8.execution_summary").unwrap();
    let mut observation = ObservationRequest::summary();
    observation.native_evidence.enabled = true;
    observation.native_evidence.kinds = vec![capability.as_str().into()];
    observation.capabilities = vec![glassvm_core::CapabilityRequest::required(
        capability.clone(),
    )];
    let mut session = prepared_session(&bundle, request(&bundle, observation));
    let mut sink = RecordingSink::default();

    session
        .execute(&mut sink)
        .expect("execute capability request");

    assert_eq!(sink.capability_outputs.len(), 1);
    assert_eq!(sink.capability_outputs[0].schema.id, capability);
    assert_eq!(sink.capability_receipts.len(), 1);
    assert_eq!(sink.capability_receipts[0].id, capability);
    assert_eq!(
        sink.capability_receipts[0].status,
        CapabilityStatus::Fulfilled
    );
}

#[test]
fn session_snapshot_restores_resumable_execution_state() {
    let bundle = Chip8Plugin::new();
    let request = request(&bundle, ObservationRequest::summary());
    let prepared_observation = bundle
        .prepare_observation(&request.observation)
        .expect("prepare observation");
    let request = request.with_prepared_observation_id(prepared_observation.identity);
    let prepared_run = bundle.prepare_run(&request).expect("prepare run");
    let mut session = bundle
        .emulator()
        .create_execution_with_prepared_run(SMOKE_ROM, request, prepared_run, prepared_observation)
        .expect("construct session");

    let initial = session.snapshot().expect("initial snapshot");
    let encoded: Value = serde_json::from_slice(&initial).expect("decode snapshot envelope");
    assert!(encoded.get("trace_history").is_none());
    session.step_frame().expect("step frame");
    let advanced = session.snapshot().expect("advanced snapshot");
    assert_ne!(advanced, initial);
    session.reset().expect("reset");
    session.restore_snapshot(&initial).expect("restore initial");
    assert_eq!(session.snapshot().expect("restored snapshot"), initial);
}

#[test]
fn incompatible_snapshot_version_is_rejected_explicitly() {
    let bundle = Chip8Plugin::new();
    let request = request(&bundle, ObservationRequest::summary());
    let mut session = prepared_session(&bundle, request);
    let snapshot = session.snapshot().expect("snapshot");
    let mut encoded: Value = serde_json::from_slice(&snapshot).expect("decode snapshot envelope");
    encoded["version"] = Value::from(4);
    let error = session
        .restore_snapshot(&serde_json::to_vec(&encoded).expect("encode old snapshot"))
        .expect_err("old snapshot version must be rejected");
    assert!(error.contains("unsupported CHIP-8 session snapshot format"));
}
