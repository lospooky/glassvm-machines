//! Static PICO-8 acceptance verification.

use crate::analysis::analyze_bytes;
use crate::artifact;
use crate::diagnostic::{Diagnostic, Severity};
use crate::report::VerificationReport;
use crate::rules::unsupported_api_calls;

pub fn verify_bytes(bytes: &[u8]) -> VerificationReport {
    let cartridge = match artifact::parse(bytes) {
        Ok(cartridge) => cartridge,
        Err(error) => {
            return VerificationReport {
                analysis: None,
                diagnostics: vec![Diagnostic {
                    code: "pico8.cartridge.invalid".into(),
                    severity: Severity::Error,
                    message: error.to_string(),
                    apis: Vec::new(),
                }],
            };
        }
    };
    let mut diagnostics = Vec::new();
    if cartridge.lua.contains("#include") {
        diagnostics.push(Diagnostic {
            code: "pico8.lua.include_unresolved".into(),
            severity: Severity::Error,
            message: "#include requires an external cartridge resolver and is not accepted by self-contained execution".into(),
            apis: Vec::new(),
        });
    }
    let patched = pico8_to_lua::patch_lua(cartridge.lua.as_str());
    let lua = mlua::Lua::new();
    if let Err(error) = lua
        .load(patched.as_ref())
        .set_name("cartridge")
        .into_function()
    {
        diagnostics.push(Diagnostic {
            code: "pico8.lua.compile_error".into(),
            severity: Severity::Error,
            message: error.to_string(),
            apis: Vec::new(),
        });
    }
    let unsupported = unsupported_api_calls(&cartridge.lua);
    if !unsupported.is_empty() {
        diagnostics.push(Diagnostic {
            code: "pico8.runtime.compatibility_subset".into(),
            severity: Severity::Warning,
            message: "cartridge references APIs outside the implemented headless subset".into(),
            apis: unsupported,
        });
    }
    diagnostics.push(Diagnostic {
        code: "pico8.runtime.numeric_model".into(),
        severity: Severity::Warning,
        message: "compatibility runtime uses Lua 5.4 host numbers rather than exact signed 16.16 PICO-8 arithmetic".into(),
        apis: Vec::new(),
    });
    VerificationReport {
        analysis: analyze_bytes(bytes).ok(),
        diagnostics,
    }
}
