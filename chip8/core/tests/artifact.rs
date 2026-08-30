use chip8_core::artifact::{MAX_ROM_BYTES, validate_rom};

#[test]
fn validates_native_rom_bounds() {
    assert!(validate_rom(&[0x00, 0xe0]).is_ok());
    assert!(validate_rom(&[]).is_err());
    assert!(validate_rom(&vec![0; MAX_ROM_BYTES + 1]).is_err());
}
