use std::collections::BTreeSet;
use std::sync::Arc;

use chip8_plugin::Chip8Plugin;
use glassvm_core::{
    Emission, EmissionSink, EventKind, EventSelection, ExecutionControls, ExecutionEvent,
    ExecutionRequest, FrameArtifact, FrameCapture, InputSchedule, MachineBundle,
    MachineConfiguration, ObservationRequest, RunResult, SinkError,
};
use hexwell_plugin::HexwellPlugin;
use pico8_plugin::Pico8Plugin;
use serde_json::Value;
use tic80_plugin::Tic80Plugin;
use wyrd16_plugin::Wyrd16Plugin;

struct BundleCase {
    bundle: Arc<dyn MachineBundle>,
    fixture: &'static [u8],
}

#[derive(Default)]
struct EvidenceSink {
    events: Vec<ExecutionEvent>,
    frames: Vec<FrameArtifact>,
}

impl EmissionSink for EvidenceSink {
    fn emit(&mut self, emission: Emission<'_>) -> Result<(), SinkError> {
        match emission {
            Emission::Event(event) => self.events.push(event.clone()),
            Emission::Frame(frame) => self.frames.push(frame.clone()),
            Emission::RunFinished(_result @ &RunResult { .. }) => {}
            _ => {}
        }
        Ok(())
    }
}

fn cases() -> Vec<BundleCase> {
    vec![
        BundleCase {
            bundle: Arc::new(Chip8Plugin::new()),
            fixture: include_bytes!("../../chip8/fixtures/smoke.rom"),
        },
        BundleCase {
            bundle: Arc::new(HexwellPlugin::new()),
            fixture: include_bytes!("../../hexwell/fixtures/smoke.rom"),
        },
        BundleCase {
            bundle: Arc::new(Wyrd16Plugin::new()),
            fixture: include_bytes!("../../wyrd16/fixtures/smoke.rom"),
        },
        BundleCase {
            bundle: Arc::new(Pico8Plugin::new()),
            fixture: include_bytes!("../../pico8/fixtures/smoke.rom"),
        },
        BundleCase {
            bundle: Arc::new(Tic80Plugin::new()),
            fixture: include_bytes!("../../tic80/fixtures/smoke.rom"),
        },
    ]
}

fn request(
    case: &BundleCase,
    run_id: &str,
    observation: ObservationRequest,
    frame_limit: u64,
) -> ExecutionRequest {
    ExecutionRequest::new(
        run_id,
        case.bundle.descriptor().id.clone(),
        case.fixture.to_vec(),
        MachineConfiguration::defaults(&case.bundle.emulator().config_schema()).unwrap(),
        InputSchedule::empty(),
        observation,
        ExecutionControls {
            frame_limit: Some(frame_limit),
            ..ExecutionControls::default()
        },
    )
    .unwrap()
}

fn session(
    case: &BundleCase,
    run_id: &str,
    observation: ObservationRequest,
    frame_limit: u64,
) -> Box<dyn glassvm_core::EmulatorSession> {
    let mut request = request(case, run_id, observation, frame_limit);
    let prepared_observation = case
        .bundle
        .prepare_observation(&request.observation)
        .unwrap_or_else(|errors| panic!("{}: {errors:?}", case.bundle.descriptor().id));
    request = request.with_prepared_observation_id(prepared_observation.identity);
    let prepared_run = case.bundle.prepare_run(&request).unwrap();
    case.bundle
        .emulator()
        .create_execution_with_prepared_run(
            case.fixture,
            request,
            prepared_run,
            prepared_observation,
        )
        .unwrap()
}

fn frame_observation(events: EventSelection) -> ObservationRequest {
    let mut observation = ObservationRequest::summary();
    observation.normalized_events.events = events;
    observation.frames.capture = FrameCapture::Hashes;
    observation
}

fn execute_frames(case: &BundleCase, events: EventSelection) -> EvidenceSink {
    let mut session = session(
        case,
        &format!("frame-contract-{}", case.bundle.descriptor().id),
        frame_observation(events),
        2,
    );
    let mut sink = EvidenceSink::default();
    session.execute(&mut sink).unwrap();
    sink
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn incompatible_snapshot(case: &BundleCase, snapshot: &[u8]) -> Vec<u8> {
    if case.bundle.descriptor().id.as_str() == "chip8" {
        let mut corrupted = snapshot.to_vec();
        corrupted[8] = 0;
        corrupted[9] = 0;
        return corrupted;
    }

    let mut value: Value = serde_json::from_slice(snapshot).unwrap();
    let version_key = if case.bundle.descriptor().id.as_str() == "hexwell" {
        "snapshot_version"
    } else {
        "version"
    };
    value[version_key] = Value::from(0);
    serde_json::to_vec(&value).unwrap()
}

#[test]
fn frame_artifacts_are_independent_of_normalized_frame_events_for_all_bundles() {
    let selected = EventSelection::Kinds(BTreeSet::from([EventKind::FrameCompleted]));
    for case in cases() {
        let with_events = execute_frames(&case, selected.clone());
        let without_events = execute_frames(&case, EventSelection::None);
        let machine = case.bundle.descriptor().id.as_str();

        assert_eq!(with_events.frames.len(), without_events.frames.len());
        assert!(
            !with_events.frames.is_empty(),
            "{machine}: no frame artifacts"
        );
        assert!(
            without_events.events.is_empty(),
            "{machine}: frame events leaked"
        );
        assert_eq!(with_events.events.len(), with_events.frames.len());

        for artifact in &with_events.frames {
            let event = with_events
                .events
                .iter()
                .find(|event| {
                    event.kind == EventKind::FrameCompleted && event.frame == Some(artifact.frame)
                })
                .unwrap_or_else(|| panic!("{machine}: no event for frame {}", artifact.frame));
            assert_eq!(event.step, artifact.step, "{machine}: frame/step mismatch");
            assert_eq!(event.run_id, artifact.run_id, "{machine}: run mismatch");
        }
    }
}

#[test]
fn state_only_snapshots_continue_in_a_fresh_session_for_all_bundles() {
    for case in cases() {
        let machine = case.bundle.descriptor().id.clone();
        let observation = ObservationRequest::summary();

        let mut source = session(
            &case,
            &format!("continuation-{machine}"),
            observation.clone(),
            3,
        );
        source.step_frame().unwrap();
        let checkpoint = source.snapshot().unwrap();
        for marker in [
            b"frame_history".as_slice(),
            b"trace_history",
            b"input_history",
        ] {
            assert!(
                !contains_bytes(&checkpoint, marker),
                "{machine}: snapshot retained {marker:?}"
            );
        }

        let mut resumed = session(
            &case,
            &format!("continuation-{machine}"),
            observation.clone(),
            3,
        );
        resumed.restore_snapshot(&checkpoint).unwrap();
        resumed.step_frame().unwrap();
        let resumed_state = resumed.snapshot().unwrap();

        let mut expected = session(&case, &format!("continuation-{machine}"), observation, 3);
        expected.step_frame().unwrap();
        expected.step_frame().unwrap();
        assert_eq!(
            resumed_state,
            expected.snapshot().unwrap(),
            "{machine}: continuation diverged"
        );
    }
}

#[test]
fn incompatible_snapshot_versions_are_rejected_for_all_bundles() {
    for case in cases() {
        let machine = case.bundle.descriptor().id.clone();
        let mut source = session(
            &case,
            &format!("snapshot-version-{machine}"),
            ObservationRequest::summary(),
            2,
        );
        source.step_frame().unwrap();
        let snapshot = source.snapshot().unwrap();
        let corrupted = incompatible_snapshot(&case, &snapshot);
        let mut target = session(
            &case,
            &format!("snapshot-version-{machine}"),
            ObservationRequest::summary(),
            2,
        );
        let error = target
            .restore_snapshot(&corrupted)
            .expect_err("incompatible snapshot version must be rejected");
        assert!(
            error.contains("unsupported") || error.contains("invalid"),
            "{machine}: unhelpful snapshot rejection: {error}"
        );
    }
}
