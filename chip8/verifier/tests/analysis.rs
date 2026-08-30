use chip8_verifier::analyze_bytes;

const ROM: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn analyzes_the_canonical_rom_without_execution() {
    let report = analyze_bytes(ROM);
    assert_eq!(report.total_count, 2);
    assert!(report.reachable_count > 0);
}
