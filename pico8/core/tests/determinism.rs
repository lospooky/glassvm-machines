use pico8_core::{Cartridge, Pico8Runtime};

const CART: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn repeated_native_runs_are_identical() {
    let cartridge = Cartridge::parse(CART).unwrap();
    let left = Pico8Runtime::new(&cartridge, 7, 500_000).unwrap();
    let right = Pico8Runtime::new(&cartridge, 7, 500_000).unwrap();
    left.step_frame().unwrap();
    right.step_frame().unwrap();
    assert_eq!(left.snapshot(), right.snapshot());
}
