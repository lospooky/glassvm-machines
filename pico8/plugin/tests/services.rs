use glassvm_core::MachineBundle;
use pico8_plugin::Pico8Plugin;

const SMOKE_CARTRIDGE: &[u8] = include_bytes!("../../fixtures/smoke.rom");

fn cartridge(lua: &str) -> Vec<u8> {
    format!("pico-8 cartridge // http://www.pico-8.com\nversion 41\n__lua__\n{lua}\n").into_bytes()
}

fn diagnostic<'a>(result: &'a glassvm_core::VerifyResult, code: &str) -> &'a serde_json::Value {
    result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic["code"] == code)
        .unwrap_or_else(|| panic!("missing diagnostic {code}: {:?}", result.diagnostics))
}

#[test]
fn public_bundle_exposes_native_analyzer_and_verifier() {
    let bundle = Pico8Plugin::new();
    let analyzer = bundle.static_analyzer().expect("PICO-8 analyzer service");
    let verifier = bundle.verifier().expect("PICO-8 verifier service");

    assert_eq!(analyzer.machine_id(), bundle.descriptor().id);
    assert_eq!(verifier.machine_id(), bundle.descriptor().id);

    let analysis = analyzer.analyze_bytes(SMOKE_CARTRIDGE).unwrap();
    assert_eq!(analysis.capabilities.len(), 1);
    assert_eq!(
        analysis.capabilities[0].schema.id.as_str(),
        "pico8.static_profile"
    );
    assert_eq!(analysis.capabilities[0].value["artifact_format"], "p8-text");
    assert_eq!(analysis.capabilities[0].value["callbacks"][0], "_init");

    let verification = verifier.verify_bytes(SMOKE_CARTRIDGE).unwrap();
    assert_eq!(verification.severity_max, "warning");
    assert_eq!(verification.capabilities, analysis.capabilities);
}

#[test]
fn malformed_artifacts_are_reported_at_the_public_service_boundary() {
    let bundle = Pico8Plugin::new();
    let analyzer_error = bundle
        .static_analyzer()
        .unwrap()
        .analyze_bytes(b"not a cartridge")
        .unwrap_err();
    assert!(analyzer_error.contains("invalid PICO-8 cartridge"));

    let verification = bundle
        .verifier()
        .unwrap()
        .verify_bytes(b"not a cartridge")
        .unwrap();
    assert_eq!(verification.severity_max, "error");
    assert!(verification.capabilities.is_empty());
    assert_eq!(
        diagnostic(&verification, "pico8.cartridge.invalid")["level"],
        "error"
    );
}

#[test]
fn unsupported_apis_and_numeric_semantics_remain_explicit() {
    let bundle = Pico8Plugin::new();
    let artifact = cartridge("function _draw()\n circ(64,64,8)\nend");
    let verification = bundle.verifier().unwrap().verify_bytes(&artifact).unwrap();

    assert_eq!(verification.severity_max, "warning");
    let unsupported = diagnostic(&verification, "pico8.runtime.compatibility_subset");
    assert_eq!(unsupported["level"], "warning");
    assert_eq!(unsupported["apis"], serde_json::json!(["circ"]));
    assert_eq!(
        diagnostic(&verification, "pico8.runtime.numeric_model")["level"],
        "warning"
    );
}

#[test]
fn static_services_compile_but_never_execute_cartridge_code() {
    let bundle = Pico8Plugin::new();
    let artifact = cartridge("while true do end\nerror('must not execute')");

    let analysis = bundle
        .static_analyzer()
        .unwrap()
        .analyze_bytes(&artifact)
        .expect("analysis must not execute the cartridge");
    let verification = bundle
        .verifier()
        .unwrap()
        .verify_bytes(&artifact)
        .expect("verification must not execute the cartridge");

    assert_eq!(analysis.capabilities.len(), 1);
    assert_eq!(verification.severity_max, "warning");
    assert!(
        verification
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic["code"] != "pico8.lua.compile_error")
    );
}
