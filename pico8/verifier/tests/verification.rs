use pico8_verifier::verify_bytes;

#[test]
fn invalid_artifact_is_a_native_error_report() {
    let report = verify_bytes(b"not a cartridge");
    assert_eq!(report.severity_max(), "error");
    assert!(report.analysis.is_none());
}
