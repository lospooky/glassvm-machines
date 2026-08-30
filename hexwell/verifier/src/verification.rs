//! Static catalyst-plate acceptance and advisory rules.

use hexwell_core::{Catalyst, Family, GRID_WIDTH, WELL_COUNT, validate_artifact};

use crate::{Diagnostic, Severity, VerificationReport};

pub fn verify_bytes(bytes: &[u8]) -> VerificationReport {
    let mut diagnostics = Vec::new();
    if let Err(error) = validate_artifact(bytes) {
        diagnostics.push(Diagnostic {
            code: "HXW_ROM_SHAPE",
            severity: Severity::Error,
            message: error.to_string(),
            well: None,
            count: None,
        });
    } else {
        let boot_well = GRID_WIDTH * 8 + 8;
        let center = Catalyst::decode(bytes[boot_well]);
        if center.family == Family::Dormant && !center.polarity() {
            diagnostics.push(Diagnostic {
                code: "HXW_BOOT_DARK",
                severity: Severity::Warning,
                message: "the boot front quenches on its first sweep unless the frame-0 tide feed ignites a portal".into(),
                well: Some(boot_well),
                count: None,
            });
        }
        let fork_count = bytes
            .iter()
            .filter(|byte| Catalyst::decode(**byte).family == Family::Fork)
            .count();
        let vent_count = bytes
            .iter()
            .filter(|byte| Catalyst::decode(**byte).family == Family::Vent)
            .count();
        if fork_count == 0 {
            diagnostics.push(Diagnostic {
                code: "HXW_NO_FORK",
                severity: Severity::Info,
                message: "the plate contains no FORK catalyst; one front can never become two"
                    .into(),
                well: None,
                count: None,
            });
        }
        if vent_count > WELL_COUNT / 4 {
            diagnostics.push(Diagnostic {
                code: "HXW_VENT_DENSE",
                severity: Severity::Info,
                message: "more than one quarter of the plate can remove matter from the reactor"
                    .into(),
                well: None,
                count: Some(vent_count),
            });
        }
    }
    let severity_max = if diagnostics
        .iter()
        .any(|item| item.severity == Severity::Error)
    {
        Severity::Error
    } else if diagnostics
        .iter()
        .any(|item| item.severity == Severity::Warning)
    {
        Severity::Warning
    } else {
        Severity::Info
    };
    VerificationReport {
        severity_max,
        diagnostics,
        isa: "hexwell-v1",
    }
}
