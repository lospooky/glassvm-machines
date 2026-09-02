use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use chip8_plugin::Chip8Plugin;
use glassvm_core::{
    Emission, EmissionSink, EventKind, EventSelection, ExecutionControls, ExecutionRequest,
    FrameCapture, InputSchedule, MachineBundle, MachineConfiguration, ObservationRequest,
    SinkError, SnapshotCapture,
};
use glassvm_recorder::{FileRunSession, RecordedRecord, RecorderLimits, ReferenceChannel};
use hexwell_plugin::HexwellPlugin;
use pico8_plugin::Pico8Plugin;
use tic80_plugin::Tic80Plugin;
use wyrd16_plugin::Wyrd16Plugin;

struct BundleCase {
    bundle: Arc<dyn MachineBundle>,
    fixture: &'static [u8],
}

#[derive(Default)]
struct CountSink {
    frames: usize,
}

impl EmissionSink for CountSink {
    fn emit(&mut self, emission: Emission<'_>) -> Result<(), SinkError> {
        if let Emission::Frame(_frame) = emission {
            self.frames += 1;
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

fn observation() -> ObservationRequest {
    let mut observation = ObservationRequest::summary();
    observation.normalized_events.events =
        EventSelection::Kinds(BTreeSet::from([EventKind::FrameCompleted]));
    observation.frames.capture = FrameCapture::Hashes;
    observation.snapshots.capture = SnapshotCapture::Final;
    observation
}

fn prepared_session(
    case: &BundleCase,
    run_id: &str,
) -> (ExecutionRequest, Box<dyn glassvm_core::EmulatorSession>) {
    let mut request = ExecutionRequest::new(
        run_id,
        case.bundle.descriptor().id.clone(),
        case.fixture.to_vec(),
        MachineConfiguration::defaults(&case.bundle.emulator().config_schema()).unwrap(),
        InputSchedule::empty(),
        observation(),
        ExecutionControls {
            frame_limit: Some(2),
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

fn output_path(machine: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("glassvm-cb32-{machine}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    path
}

fn read_all(
    path: &glassvm_recorder::PublishedFileRun,
    channel: ReferenceChannel,
) -> Vec<RecordedRecord> {
    let paths = path.segment_paths(channel.clone()).unwrap();
    paths
        .into_iter()
        .enumerate()
        .flat_map(|(index, _)| {
            let mut reader = path.open_segment(channel.clone(), index as u64).unwrap();
            let mut records = Vec::new();
            while let Some(record) = reader.next_record().unwrap() {
                records.push(record);
            }
            records
        })
        .collect()
}

#[test]
fn all_five_publish_selective_channels_with_separate_receipts_and_round_trips() {
    for case in cases() {
        let machine = case.bundle.descriptor().id.as_str();
        let path = output_path(machine);
        let (request, mut session) = prepared_session(&case, &format!("durable-{machine}"));
        let result = FileRunSession::run(
            &mut *session,
            &request,
            &path,
            case.bundle.descriptor().machine_version.clone(),
            RecorderLimits {
                max_block_logical_bytes: 1_048_576,
                max_segment_records: 2,
                max_segment_logical_bytes: 2_097_152,
                max_buffered_bytes_per_channel: 1_048_576,
            },
        )
        .unwrap_or_else(|error| panic!("{machine}: file run failed: {error}"));

        assert!(
            result.execution.common.boot_success,
            "{machine}: execution failed"
        );
        assert_eq!(
            result.evidence.status,
            glassvm_core::EvidenceStatus::Complete
        );
        assert!(
            result.recorder.finalized,
            "{machine}: recorder was not finalized"
        );
        assert!(result.recorder.evidence_receipt_published);
        assert!(result.recorder.is_publishable());
        assert!(path.is_dir(), "{machine}: atomic publication path missing");

        let published = &result.published;
        let normalized = read_all(published, ReferenceChannel::NormalizedEvents);
        let frames = read_all(published, ReferenceChannel::Frames);
        let snapshots = read_all(published, ReferenceChannel::Snapshots);
        assert!(
            !normalized.is_empty(),
            "{machine}: normalized channel did not round-trip"
        );
        assert!(
            !frames.is_empty(),
            "{machine}: frame channel did not round-trip"
        );
        assert!(
            !snapshots.is_empty(),
            "{machine}: snapshot channel did not round-trip"
        );
        assert!(
            read_all(published, ReferenceChannel::NativeEvidence).is_empty(),
            "{machine}: unrequested native channel was materialized"
        );
        assert!(
            read_all(published, ReferenceChannel::CapabilityOutputs).is_empty(),
            "{machine}: unrequested capability channel was materialized"
        );
        assert_eq!(
            published.recorder_receipt().evidence_status,
            published.evidence_receipt().status
        );

        let _ = fs::remove_dir_all(path);
    }
}

#[test]
fn file_backed_lifecycle_does_not_require_whole_run_materialization() {
    for case in cases() {
        let machine = case.bundle.descriptor().id.as_str();
        let (request, mut session) = prepared_session(&case, &format!("streaming-{machine}"));
        let mut sink = CountSink::default();
        let result = session.execute(&mut sink).unwrap();
        assert!(result.common.boot_success, "{machine}: execution failed");
        assert!(sink.frames > 0, "{machine}: no frame boundary reached");
        assert_eq!(request.observation.frames.capture, FrameCapture::Hashes);
    }
}
