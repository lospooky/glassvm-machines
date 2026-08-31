use pico8_core::{Cartridge, CartridgeFormat};

const CART: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn loads_the_native_text_cartridge() {
    let cartridge = Cartridge::parse(CART).unwrap();
    assert_eq!(cartridge.format, CartridgeFormat::P8Text);
    assert!(!cartridge.lua.is_empty());
}
