use wyrd16_verifier::{Severity, verify_bytes};

#[test]
fn verifier_reports_shape_errors_but_accepts_every_aligned_word() {
    assert_eq!(verify_bytes(&[0x10]).severity_max, Severity::Error);
    assert_ne!(verify_bytes(&[0xff, 0xff]).severity_max, Severity::Error);
}
