use std::collections::BTreeMap;

use glassvm_core::{ExecutionRequest, MachineBundle, ObservationPlan, RunConfig, TraceCollector};
use pico8_plugin::Pico8Plugin;

const SMOKE: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn canonical_smoke_cartridge_executes_through_the_plugin() {
    let plugin = Pico8Plugin::new();
    let request = ExecutionRequest::new(
        "pico8-plugin-emulation",
        "pico8",
        RunConfig {
            max_frames: 1,
            cycles_per_frame: 100,
            seed: 17,
            machine_params: BTreeMap::new(),
            record_frames: true,
            record_events: true,
        },
        ObservationPlan::ui(),
    )
    .expect("execution request");
    let mut session = plugin
        .emulator()
        .create_execution(SMOKE, request)
        .expect("PICO-8 session");

    let result = session
        .execute(&mut TraceCollector::new())
        .expect("execute smoke cartridge");

    assert_eq!(result.common.frames, 1);
    assert!(result.common.boot_success);
    assert!(
        result
            .capabilities
            .iter()
            .any(|capability| capability.key == "display.framebuffer_flat")
    );
}
