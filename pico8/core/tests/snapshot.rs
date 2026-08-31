use pico8_core::{Cartridge, Pico8Runtime, RAM_BYTES};

const CART: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn native_snapshot_captures_complete_visible_machine_state() {
    let cartridge = Cartridge::parse(CART).unwrap();
    let runtime = Pico8Runtime::new(&cartridge, 13, 500_000).unwrap();
    runtime.step_frame().unwrap();
    let snapshot = runtime.snapshot();
    assert_eq!(snapshot.frame, 1);
    assert_eq!(snapshot.ram.len(), RAM_BYTES);
}
