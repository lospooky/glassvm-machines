use glassvm_core::{
    EventSelection, ExecutionControls, ExecutionRequest, InputSchedule, MachineBundle,
    MachineConfiguration, ObservationRequest,
};
use glassvm_recorder::{
    FileRunSession, MachineExecutionOutcome, PublicationOutcome, RecorderFinalizationOutcome,
    RecorderLimits,
};
use std::sync::Arc;

#[test]
fn guest_faults_complete_the_lifecycle_and_preserve_diagnostics() {
    let lua = b"-- script: lua\nfunction TIC() error('E1 F9 runtime fault') end\n";
    let mut tic = vec![5, lua.len() as u8, (lua.len() >> 8) as u8, 0];
    tic.extend_from_slice(lua);
    let cases: Vec<(Arc<dyn MachineBundle>, Vec<u8>, &str)> = vec![
        (Arc::new(chip8_bundle::Chip8Plugin::new()), vec![0xff,0xff], "chip8.invalid_opcode"),
        (Arc::new(pico8_bundle::Pico8Plugin::new()), b"pico-8 cartridge // http://www.pico-8.com\nversion 41\n__lua__\nfunction _draw() error('E1 F9 runtime fault') end\n".to_vec(), "pico8.callback_error"),
        (Arc::new(tic80_bundle::Tic80Plugin::new()), tic, "tic80.callback_error"),
    ];
    for (bundle, artifact, code) in cases {
        let mut obs = ObservationRequest::summary();
        obs.normalized_events.events = EventSelection::All;
        let mut req = ExecutionRequest::new(
            format!("f9-{}", bundle.descriptor().id),
            bundle.descriptor().id.clone(),
            artifact.clone(),
            MachineConfiguration::defaults(&bundle.emulator().config_schema()).unwrap(),
            InputSchedule::empty(),
            obs,
            ExecutionControls {
                frame_limit: Some(2),
                ..ExecutionControls::default()
            },
        )
        .unwrap();
        let po = bundle.prepare_observation(&req.observation).unwrap();
        req = req.with_prepared_observation_id(po.identity);
        let pr = bundle.prepare_run(&req).unwrap();
        let mut session = bundle
            .emulator()
            .create_execution_with_prepared_run(&artifact, req.clone(), pr, po)
            .unwrap();
        let parent = std::env::temp_dir().join(format!(
            "glassvm-f9-{}-{}",
            bundle.descriptor().id,
            std::process::id()
        ));
        std::fs::create_dir_all(&parent).unwrap();
        let result = FileRunSession::run(
            &mut *session,
            &req,
            parent.join("published"),
            bundle.descriptor().machine_version.clone(),
            RecorderLimits {
                max_block_logical_bytes: 1024 * 1024,
                max_segment_records: 1024,
                max_segment_logical_bytes: 1024 * 1024,
                max_buffered_bytes_per_channel: 1024 * 1024,
            },
        )
        .unwrap();
        let MachineExecutionOutcome::Completed(run) = result.machine_execution else {
            panic!("{} did not complete session", bundle.descriptor().id)
        };
        let fault = run.fault.expect("machine fault");
        assert_eq!(fault.code, code);
        if code.ends_with("callback_error") {
            assert!(fault.message.contains("E1 F9 runtime fault"));
        }
        assert_eq!(
            result.evidence_fulfillment.status,
            glassvm_core::EvidenceStatus::Complete
        );
        assert!(matches!(
            result.recorder_finalization,
            RecorderFinalizationOutcome::Finalized(_)
        ));
        assert!(matches!(result.publication, PublicationOutcome::Published));
        std::fs::remove_dir_all(parent).unwrap();
    }
}
