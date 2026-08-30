//! Wyrd-16-specific static verification rules.

use crate::{Diagnostic, InspectedArtifact, Severity};

pub fn inspect_runes(artifact: &InspectedArtifact, diagnostics: &mut Vec<Diagnostic>) {
    for (index, rune) in artifact.runes.iter().enumerate() {
        if rune.op == 0 && rune.addr12() > 3 {
            diagnostics.push(Diagnostic {
                code: "W16_RESERVED_CHARM",
                level: Severity::Info,
                address: Some(index * 2),
                message: "reserved CHARM executes as a defined no-op in v1".into(),
            });
        }
        if rune.op == 0x7 && rune.addr12() & 1 != 0 {
            diagnostics.push(Diagnostic {
                code: "W16_JUMP_ALIGN",
                level: Severity::Info,
                address: Some(index * 2),
                message: "odd JUMP target is canonically rounded down".into(),
            });
        }
    }
}
