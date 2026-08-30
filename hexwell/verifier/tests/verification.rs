use hexwell_verifier::{Severity, verify_bytes};

const ROM: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn malformed_plate_is_rejected() {
    assert_ne!(verify_bytes(ROM).severity_max, Severity::Error);
    assert_eq!(verify_bytes(&[]).severity_max, Severity::Error);
}
