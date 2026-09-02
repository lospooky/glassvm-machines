use std::sync::Arc;

use chip8_plugin::Chip8Plugin;
use glassvm_core::{
    BudgetedSink, ChannelStatus, Emission, EmissionBudgets, EmissionChannel, EmissionSink,
    EventSelection, EvidenceReceipt, EvidenceStatus, ExecutionControls, ExecutionEvent,
    ExecutionRequest, FrameArtifact, FrameCapture, InputSchedule, MachineBundle,
    MachineConfiguration, ObservationRequest, OverflowPolicy, RunResult, SinkError,
};
use hexwell_plugin::HexwellPlugin;
use pico8_plugin::Pico8Plugin;
use tic80_plugin::Tic80Plugin;
use wyrd16_plugin::Wyrd16Plugin;

struct BundleCase {
    bundle: Arc<dyn MachineBundle>,
    fixture: &'static [u8],
}

#[derive(Default)]
struct RecordingSink {
    events: Vec<ExecutionEvent>,
    frames: Vec<FrameArtifact>,
    native_events: usize,
    receipts: Vec<EvidenceReceipt>,
}

impl EmissionSink for RecordingSink {
    fn emit(&mut self, emission: Emission<'_>) -> Result<(), SinkError> {
        match emission {
            Emission::Event(event) => self.events.push(event.clone()),
            Emission::Frame(frame) => self.frames.push(frame.clone()),
            Emission::NativeEvidence(_) => self.native_events += 1,
            Emission::EvidenceReceipt(receipt) => self.receipts.push(receipt.clone()),
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

fn session(
    case: &BundleCase,
    run_id: &str,
    observation: ObservationRequest,
    frame_limit: u64,
) -> (ExecutionRequest, Box<dyn glassvm_core::EmulatorSession>) {
    let mut request = ExecutionRequest::new(
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
    .unwrap();
    let prepared_observation = case
        .bundle
        .prepare_observation(&request.observation)
        .unwrap();
    request = request.with_prepared_observation_id(prepared_observation.identity);
    let prepared_run = case.bundle.prepare_run(&request).unwrap();
    let session = case
        .bundle
        .emulator()
        .create_execution_with_prepared_run(
            case.fixture,
            request.clone(),
            prepared_run,
            prepared_observation,
        )
        .unwrap();
    (request, session)
}

fn run_bounded(
    case: &BundleCase,
    observation: ObservationRequest,
) -> (RunResult, EvidenceReceipt, RecordingSink) {
    let (request, mut session) = session(
        case,
        &format!("boundedness-{}", case.bundle.descriptor().id),
        observation,
        2,
    );
    let mut budgeted = BudgetedSink::new(
        RecordingSink::default(),
        request.observation.clone(),
        request.run_id.clone(),
        request.machine_id.clone(),
        case.bundle.descriptor().machine_version.clone(),
    )
    .unwrap();
    let result = session.execute(&mut budgeted).unwrap();
    let receipt = budgeted.emit_receipt().unwrap();
    (result, receipt, budgeted.into_inner())
}

fn channel(receipt: &EvidenceReceipt, expected: EmissionChannel) -> &glassvm_core::ChannelReceipt {
    receipt
        .channels
        .iter()
        .find(|channel| channel.channel == expected)
        .unwrap()
}

fn incomplete_budget() -> EmissionBudgets {
    EmissionBudgets {
        max_normalized_events: Some(0),
        ..EmissionBudgets::default()
    }
}

#[test]
fn normalized_event_budget_is_hard_and_does_not_change_machine_outcome() {
    for case in cases() {
        let mut observation = ObservationRequest::summary();
        observation.normalized_events.events = EventSelection::All;
        observation.budgets = incomplete_budget();
        observation.overflow = OverflowPolicy::AllowIncomplete;
        let (result, receipt, sink) = run_bounded(&case, observation);
        let machine = case.bundle.descriptor().id.as_str();

        assert!(
            result.common.boot_success,
            "{machine}: machine outcome changed"
        );
        assert_eq!(receipt.status, EvidenceStatus::Incomplete);
        let normalized = channel(&receipt, EmissionChannel::NormalizedEvents);
        assert_eq!(normalized.status, ChannelStatus::Truncated);
        assert_eq!(normalized.item_count, 0);
        assert!(
            sink.events.is_empty(),
            "{machine}: over-budget event forwarded"
        );
        assert_eq!(
            sink.receipts.len(),
            1,
            "{machine}: receipt was not emitted once"
        );
    }
}

#[test]
fn frame_budget_rejects_an_oversized_record_without_overrunning_the_channel() {
    for case in cases() {
        let mut observation = ObservationRequest::summary();
        observation.frames.capture = FrameCapture::Hashes;
        observation.budgets.max_frame_bytes = Some(1);
        observation.overflow = OverflowPolicy::AllowIncomplete;
        let (result, receipt, sink) = run_bounded(&case, observation);
        let machine = case.bundle.descriptor().id.as_str();

        assert!(
            result.common.boot_success,
            "{machine}: machine outcome changed"
        );
        assert_eq!(receipt.status, EvidenceStatus::Incomplete);
        let frames = channel(&receipt, EmissionChannel::Frames);
        assert_eq!(frames.status, ChannelStatus::Truncated);
        assert_eq!(frames.item_count, 0);
        assert!(
            sink.frames.is_empty(),
            "{machine}: oversized frame forwarded"
        );
    }
}

#[test]
fn snapshot_budget_is_independent_and_hard_for_every_bundle() {
    for case in cases() {
        let mut observation = ObservationRequest::summary();
        observation.snapshots.capture = glassvm_core::SnapshotCapture::Final;
        observation.budgets.max_snapshot_bytes = Some(1);
        observation.overflow = OverflowPolicy::AllowIncomplete;
        let (result, receipt, _sink) = run_bounded(&case, observation);
        let machine = case.bundle.descriptor().id.as_str();

        assert!(
            result.common.boot_success,
            "{machine}: machine outcome changed"
        );
        assert_eq!(receipt.status, EvidenceStatus::Incomplete);
        let snapshots = channel(&receipt, EmissionChannel::Snapshots);
        assert_eq!(snapshots.status, ChannelStatus::Truncated);
        assert_eq!(snapshots.item_count, 0);
    }
}

#[test]
fn native_event_budget_is_hard_and_channel_local_for_every_bundle() {
    for case in cases() {
        let mut observation = ObservationRequest::summary();
        observation.native_evidence.enabled = true;
        observation.native_evidence.all = true;
        observation.budgets.max_native_events = Some(0);
        observation.overflow = OverflowPolicy::AllowIncomplete;
        let (result, receipt, sink) = run_bounded(&case, observation);
        let machine = case.bundle.descriptor().id.as_str();

        assert!(
            result.common.boot_success,
            "{machine}: machine outcome changed"
        );
        assert_eq!(receipt.status, EvidenceStatus::Incomplete);
        let native = channel(&receipt, EmissionChannel::NativeEvidence);
        assert_eq!(native.status, ChannelStatus::Truncated);
        assert_eq!(native.item_count, 0);
        assert_eq!(
            sink.native_events, 0,
            "{machine}: native overflow forwarded"
        );
    }
}
