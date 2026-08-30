use wyrd16_verifier::analyze_bytes;

const FIXTURE: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn smoke_artifact_has_the_published_static_profile() {
    let report = analyze_bytes(FIXTURE).expect("analysis");
    assert_eq!(report.structure.rune_count, 5);
    assert_eq!(report.drawing_runes, 1);
    assert_eq!(report.opcode_counts[0xC], 1);
}
