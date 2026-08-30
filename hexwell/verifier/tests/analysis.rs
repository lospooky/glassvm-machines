use hexwell_verifier::analyze_bytes;

const ROM: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn checked_in_plate_has_a_static_profile() {
    let report = analyze_bytes(ROM).unwrap();
    assert_eq!(report.structure.catalyst_count, 256);
}
