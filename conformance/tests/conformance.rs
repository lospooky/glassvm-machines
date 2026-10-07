use std::sync::Arc;

use chip8_bundle::Chip8Plugin;
use glassvm_core::{
    CapabilityId, CapabilityRequest, Emission, EmissionSink, ExecutionControls, ExecutionEvent,
    ExecutionRequest, FrameArtifact, InputSchedule, MachineBundle, MachineConfiguration,
    ObservationRequest, PreparedCapabilityStatus, RunResult, SinkError,
};
use pico8_bundle::Pico8Plugin;
use tic80_bundle::Tic80Plugin;

struct BundleCase {
    bundle: Arc<dyn MachineBundle>,
    fixture: &'static [u8],
}

#[derive(Default)]
struct RecordingSink {
    started: bool,
    finished: bool,
    events: Vec<ExecutionEvent>,
    frames: Vec<FrameArtifact>,
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

fn cases() -> Vec<BundleCase> {
    vec![
        BundleCase {
            bundle: Arc::new(Chip8Plugin::new()),
            fixture: include_bytes!("../../chip8/fixtures/smoke.rom"),
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

fn request(case: &BundleCase, run_id: &str, observation: ObservationRequest) -> ExecutionRequest {
    ExecutionRequest::new(
        run_id,
        case.bundle.descriptor().id.clone(),
        case.fixture.to_vec(),
        MachineConfiguration::defaults(&case.bundle.emulator().config_schema())
            .expect("bundle configuration defaults"),
        InputSchedule::empty(),
        observation,
        ExecutionControls {
            frame_limit: Some(1),
            ..ExecutionControls::default()
        },
    )
    .expect("publication request")
}

fn prepare(
    case: &BundleCase,
    run_id: &str,
    observation: ObservationRequest,
) -> (
    ExecutionRequest,
    glassvm_core::PreparedRun,
    glassvm_core::PreparedObservation,
) {
    let mut request = request(case, run_id, observation);
    let observation_request = request.observation.clone();
    let prepared_observation = case
        .bundle
        .prepare_observation(&observation_request)
        .unwrap_or_else(|errors| {
            panic!(
                "{}: observation preparation failed: {errors:?}",
                case.bundle.descriptor().id
            )
        });
    request = request.with_prepared_observation_id(prepared_observation.identity);
    let prepared_run = case.bundle.prepare_run(&request).unwrap_or_else(|error| {
        panic!(
            "{}: run preparation failed: {error}",
            case.bundle.descriptor().id
        )
    });
    (request, prepared_run, prepared_observation)
}

#[test]
fn one_machine_independent_runner_prepares_and_executes_all_clean_core_bundles() {
    for case in cases() {
        let machine = case.bundle.descriptor().id.clone();
        let (request, prepared_run, prepared_observation) = prepare(
            &case,
            &format!("shared-runner-{machine}"),
            ObservationRequest::summary(),
        );
        let mut session = case
            .bundle
            .emulator()
            .create_execution_with_prepared_run(
                case.fixture,
                request,
                prepared_run,
                prepared_observation,
            )
            .unwrap_or_else(|error| panic!("{machine}: session construction failed: {error}"));
        let mut sink = RecordingSink::default();
        let result = session
            .execute(&mut sink)
            .unwrap_or_else(|error| panic!("{machine}: execution failed: {error}"));
        assert!(result.common.boot_success, "{machine}: boot failed");
        assert!(sink.started, "{machine}: missing RunStarted");
        assert!(sink.finished, "{machine}: missing RunFinished");
    }
}

#[test]
fn required_unknown_capability_fails_before_execution_preparation() {
    for case in cases() {
        let machine = case.bundle.descriptor().id.clone();
        let mut observation = ObservationRequest::summary();
        let id = CapabilityId::new(format!("{machine}.missing.v1")).expect("canonical ID");
        observation
            .capabilities
            .push(CapabilityRequest::required(id));

        let request = request(&case, &format!("required-failure-{machine}"), observation);
        let observation_request = request.observation.clone();
        let error = case
            .bundle
            .prepare_observation(&observation_request)
            .expect_err("unknown required capability must fail preparation");
        assert!(
            error.iter().any(|message| message.contains("not declared")),
            "{machine}: missing required-capability reason: {error:?}"
        );
    }
}

#[test]
fn optional_unknown_capability_is_explicitly_unavailable() {
    for case in cases() {
        let machine = case.bundle.descriptor().id.clone();
        let mut observation = ObservationRequest::summary();
        let id = CapabilityId::new(format!("{machine}.optional-missing.v1")).expect("canonical ID");
        observation
            .capabilities
            .push(CapabilityRequest::optional(id.clone()));

        let request = request(&case, &format!("optional-downgrade-{machine}"), observation);
        let observation_request = request.observation.clone();
        let prepared = case
            .bundle
            .prepare_observation(&observation_request)
            .unwrap_or_else(|errors| panic!("{machine}: optional capability failed: {errors:?}"));
        let capability = prepared
            .capabilities
            .iter()
            .find(|capability| capability.id == id)
            .expect("optional capability is represented");
        assert_eq!(capability.status, PreparedCapabilityStatus::Unavailable);
        assert!(capability.message.is_some(), "{machine}: missing reason");
    }
}

#[test]
fn observation_changes_do_not_change_prepared_execution_identity() {
    for case in cases() {
        let machine = case.bundle.descriptor().id.clone();
        let (_, empty_run, _) = prepare(
            &case,
            &format!("identity-empty-{machine}"),
            ObservationRequest::summary(),
        );
        let (_, debug_run, _) = prepare(
            &case,
            &format!("identity-debug-{machine}"),
            ObservationRequest::debug(),
        );
        assert_eq!(
            empty_run.identity, debug_run.identity,
            "{machine}: observation leaked into execution identity"
        );
    }
}
