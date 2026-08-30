//! Artifact acceptance and diagnostic production.

use wyrd16_core::validate_artifact;

use crate::{Diagnostic, InspectedArtifact, Severity, VerificationReport, rules};

pub fn verify_bytes(bytes: &[u8]) -> VerificationReport {
    let mut diagnostics = Vec::new();
    if let Err(error) = validate_artifact(bytes) {
        diagnostics.push(Diagnostic {
            code: "W16_ROM_SHAPE",
            level: Severity::Error,
            address: None,
            message: error.to_string(),
        });
    } else {
        rules::inspect_runes(&InspectedArtifact::inspect(bytes), &mut diagnostics);
    }
    let severity_max = if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.level == Severity::Error)
    {
        Severity::Error
    } else {
        Severity::Info
    };
    VerificationReport {
        severity_max,
        diagnostics,
        isa: "wyrd16-v1",
    }
}
