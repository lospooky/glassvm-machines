use tic80_verifier::analyze_bytes;

const SMOKE: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn analyzes_the_upstream_tic_cartridge_without_executing_it() {
    let report = analyze_bytes(SMOKE).expect("analyze native .tic cartridge");
    assert_eq!(report.language, "lua");
    assert!(report.has_tic_callback);
    assert!(!report.chunks.is_empty());
}
