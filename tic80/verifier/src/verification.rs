//! Static TIC-80 acceptance policy.

use crate::analysis::analyze_bytes;
use crate::artifact;
use crate::diagnostic::{Diagnostic, Severity};
use crate::report::VerificationReport;
use crate::rules::{compile_without_execution, contains_callback};

pub fn verify_bytes(bytes: &[u8]) -> VerificationReport {
    let cartridge = match artifact::parse(bytes) {
        Ok(cartridge) => cartridge,
        Err(error) => {
            return VerificationReport {
                analysis: None,
                diagnostics: vec![Diagnostic {
                    code: "invalid_cartridge".into(),
                    severity: Severity::Error,
                    message: error,
                }],
            };
        }
    };

    let mut diagnostics = Vec::new();
    if cartridge.language != "lua" {
        diagnostics.push(Diagnostic {
            code: "unsupported_language".into(),
            severity: Severity::Error,
            message: format!("bundle executes Lua carts, found {}", cartridge.language),
        });
    }
    let compilation = compile_without_execution(&cartridge.code);
    if compilation.is_err() || !contains_callback(&cartridge.code, "TIC") {
        diagnostics.push(Diagnostic {
            code: "missing_tic_callback".into(),
            severity: Severity::Error,
            message: "cartridge source does not define a TIC callback".into(),
        });
    }
    if cartridge.language == "lua"
        && let Err(error) = compilation
    {
        diagnostics.push(Diagnostic {
            code: "lua_compile_error".into(),
            severity: Severity::Error,
            message: error,
        });
    }

    VerificationReport {
        analysis: analyze_bytes(bytes).ok(),
        diagnostics,
    }
}
