//! E1 F4: reject the first normalized event through the public Rust sink.
use glassvm_core::{
    BudgetedSink, Emission, EmissionSink, EventSelection, ExecutionControls, ExecutionRequest,
    InputSchedule, MachineBundle, MachineConfiguration, ObservationRequest, SinkError,
};
use serde_json::json;
use std::sync::Arc;
struct RejectEvent {
    rejected: usize,
}
impl EmissionSink for RejectEvent {
    fn emit(&mut self, emission: Emission<'_>) -> Result<(), SinkError> {
        if matches!(emission, Emission::Event(_)) {
            self.rejected += 1;
            return Err(SinkError::new("E1 F4 normalized event rejected"));
        }
        Ok(())
    }
}
fn main() {
    let machine = std::env::args().nth(1).expect("machine id");
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
        format!("e1-f4-{machine}"),
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
    let mut sink = BudgetedSink::new(
        RejectEvent { rejected: 0 },
        request.observation.clone(),
        request.run_id.clone(),
        request.machine_id.clone(),
        bundle.descriptor().machine_version.clone(),
    )
    .unwrap();
    let execution = session.execute(&mut sink);
    let receipt = sink.receipt();
    let rejected = sink.into_inner().rejected;
    assert_eq!(rejected, 1, "must reach rejection");
    assert!(execution.is_err(), "rejected event must interrupt session");
    assert_eq!(receipt.status, glassvm_core::EvidenceStatus::Failed);
    assert!(
        receipt
            .failure
            .as_ref()
            .unwrap()
            .message
            .contains("E1 F4 normalized event rejected")
    );
    println!("{}",serde_json::to_string(&json!({"case":"F4","machine":machine,"fixture_bytes":fixture.len(),"request":request,"execution_error":execution.err(),"rejected_events":rejected,"evidence_receipt":receipt,"recorder":"not_requested","publication":"not_requested"})).unwrap());
}
