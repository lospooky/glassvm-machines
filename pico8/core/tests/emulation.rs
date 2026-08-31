use pico8_core::{Cartridge, Pico8Runtime};

const CART: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn executes_a_native_callback_frame() {
    let cartridge = Cartridge::parse(CART).unwrap();
    let runtime = Pico8Runtime::new(&cartridge, 11, 500_000).unwrap();
    runtime.step_frame().unwrap();
    assert_eq!(runtime.frame(), 1);
    assert_eq!(runtime.framebuffer().len(), 128 * 128);
}
