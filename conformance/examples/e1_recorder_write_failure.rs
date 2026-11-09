//! E1 F8-W: reject a recorder write through its public hard block limit.
use glassvm_core::{
    EventSelection, ExecutionControls, ExecutionRequest, InputSchedule, MachineBundle,
    MachineConfiguration, ObservationRequest,
};
use glassvm_recorder::{
    FileRunSession, MachineExecutionOutcome, PublicationOutcome, RecorderFinalizationOutcome,
    RecorderLimits,
};
use serde_json::json;
use std::{path::PathBuf, sync::Arc};
fn main() {
    let mut args = std::env::args().skip(1);
    let machine = args.next().expect("machine");
    let path = PathBuf::from(args.next().expect("output path"));
    let (bundle, fixture): (Arc<dyn MachineBundle>, &[u8]) = match machine.as_str() {
        "chip8" => (
            Arc::new(chip8_bundle::Chip8Plugin::new()),
            include_bytes!("../../chip8/fixtures/smoke.rom"),
        ),
        "pico8" => (
            Arc::new(pico8_bundle::Pico8Plugin::new()),
            include_bytes!("../../pico8/fixtures/smoke.rom"),
        ),
        "tic80" => (
            Arc::new(tic80_bundle::Tic80Plugin::new()),
            include_bytes!("../../tic80/fixtures/smoke.rom"),
        ),
        _ => panic!("unknown machine"),
    };
    let mut observation = ObservationRequest::summary();
    observation.normalized_events.events = EventSelection::All;
    let mut request = ExecutionRequest::new(
        format!("e1-f8w-{machine}"),
        bundle.descriptor().id.clone(),
        fixture.to_vec(),
        MachineConfiguration::defaults(&bundle.emulator().config_schema()).unwrap(),
        InputSchedule::empty(),
        observation,
        ExecutionControls {
            frame_limit: Some(2),
            ..ExecutionControls::default()
        },
    )
    .unwrap();
    let prepared_observation = bundle.prepare_observation(&request.observation).unwrap();
    request = request.with_prepared_observation_id(prepared_observation.identity);
    let prepared_run = bundle.prepare_run(&request).unwrap();
    let mut session = bundle
        .emulator()
        .create_execution_with_prepared_run(
            fixture,
            request.clone(),
            prepared_run,
            prepared_observation,
        )
        .unwrap();
    let limits = RecorderLimits {
        max_block_logical_bytes: 1,
        max_segment_records: 1,
        max_segment_logical_bytes: 1,
        max_buffered_bytes_per_channel: 1,
    };
    let result = FileRunSession::run(
        &mut *session,
        &request,
        &path,
        bundle.descriptor().machine_version.clone(),
        limits,
    )
    .unwrap();
    assert!(matches!(
        result.machine_execution,
        MachineExecutionOutcome::Completed(_)
    ));
    assert_eq!(
        result.evidence_fulfillment.status,
        glassvm_core::EvidenceStatus::Complete
    );
    assert!(matches!(
        result.recorder_finalization,
        RecorderFinalizationOutcome::Failed { .. }
    ));
    assert!(matches!(
        result.publication,
        PublicationOutcome::NotAttempted { .. }
    ));
    assert!(result.published.is_none());
    println!("{}",serde_json::to_string(&json!({"case":"F8-W","machine":machine,"request":request,"limits":{"max_block_logical_bytes":1,"max_segment_records":1,"max_segment_logical_bytes":1,"max_buffered_bytes_per_channel":1},"result":result,"published_exists":path.exists()})).unwrap());
}
