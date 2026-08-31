use pico8_verifier::analyze_bytes;

const CART: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn analyzes_native_smoke_cartridge() {
    let report = analyze_bytes(CART).unwrap();
    assert_eq!(report.artifact_format, "p8-text");
    assert!(report.callbacks.iter().any(|name| name == "_draw"));
}
