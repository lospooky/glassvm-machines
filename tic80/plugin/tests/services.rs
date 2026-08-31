use std::collections::BTreeMap;

use glassvm_core::{
    ExecutionControls, ExecutionRequest, InputSchedule, MachineBundle, MachineConfiguration,
    ObservationRequest, StructuredValue,
};
use tic80_plugin::Tic80Plugin;

const SMOKE_CARTRIDGE: &[u8] = include_bytes!("../../fixtures/smoke.rom");

fn lua_cart(source: &str) -> Vec<u8> {
    let mut cart = vec![5];
    cart.extend_from_slice(&(source.len() as u16).to_le_bytes());
    cart.push(0);
    cart.extend_from_slice(source.as_bytes());
    cart
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
    let bundle = Tic80Plugin::new();
    let analyzer = bundle.static_analyzer().expect("TIC-80 analyzer service");
    let verifier = bundle.verifier().expect("TIC-80 verifier service");

    assert_eq!(analyzer.machine_id(), bundle.descriptor().id);
    assert_eq!(verifier.machine_id(), bundle.descriptor().id);

    let analysis = analyzer.analyze_bytes(SMOKE_CARTRIDGE).unwrap();
    assert_eq!(analysis.capabilities.len(), 1);
    assert_eq!(
        analysis.capabilities[0].schema.id.as_str(),
        "tic80.static_profile"
    );
    assert_eq!(analysis.capabilities[0].value["language"], "lua");
    assert_eq!(analysis.capabilities[0].value["has_tic_callback"], true);
    assert!(analysis.capabilities[0].value.get("sha256").is_none());

    let verification = verifier.verify_bytes(SMOKE_CARTRIDGE).unwrap();
    assert_eq!(verification.severity_max, "ok");
    assert!(verification.diagnostics.is_empty());
    assert_eq!(verification.capabilities, analysis.capabilities);
}

#[test]
fn malformed_artifacts_are_reported_at_the_public_service_boundary() {
    let bundle = Tic80Plugin::new();
    let analyzer_error = bundle
        .static_analyzer()
        .unwrap()
        .analyze_bytes(b"bad")
        .unwrap_err();
    assert!(analyzer_error.contains("truncated chunk header"));

    let verification = bundle.verifier().unwrap().verify_bytes(b"bad").unwrap();
    assert_eq!(verification.severity_max, "error");
    assert!(verification.capabilities.is_empty());
    assert_eq!(
        diagnostic(&verification, "invalid_cartridge")["level"],
        "error"
    );
}

#[test]
fn unsupported_languages_and_runtime_configuration_fail_explicitly() {
    let bundle = Tic80Plugin::new();
    let javascript = lua_cart("// script: javascript\nfunction TIC() {}");
    let analysis = bundle
        .static_analyzer()
        .unwrap()
        .analyze_bytes(&javascript)
        .unwrap();
    assert_eq!(analysis.capabilities[0].value["language"], "javascript");
    let verification = bundle
        .verifier()
        .unwrap()
        .verify_bytes(&javascript)
        .unwrap();
    assert_eq!(verification.severity_max, "error");
    assert_eq!(
        diagnostic(&verification, "unsupported_language")["level"],
        "error"
    );

    let configuration = MachineConfiguration::new(
        bundle.emulator().config_schema().schema,
        StructuredValue::Map(BTreeMap::from([
            ("cycles_per_frame".into(), StructuredValue::Unsigned(1)),
            ("runtime".into(), StructuredValue::Text("wren".into())),
            ("machine_seed".into(), StructuredValue::Unsigned(0)),
        ])),
    )
    .unwrap();
    let request = ExecutionRequest::new(
        "tic80-unsupported-runtime",
        bundle.descriptor().id.clone(),
        SMOKE_CARTRIDGE.to_vec(),
        configuration,
        InputSchedule::empty(),
        ObservationRequest::summary(),
        ExecutionControls {
            frame_limit: Some(1),
            ..ExecutionControls::default()
        },
    )
    .unwrap();
    let error = bundle.prepare_run(&request).unwrap_err();
    assert_eq!(
        error,
        "configuration field \"runtime\" has an invalid value"
    );
}

#[test]
fn static_services_compile_but_never_execute_cartridge_code() {
    let bundle = Tic80Plugin::new();
    let artifact = lua_cart("while true do end\nfunction TIC() end");

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

    assert_eq!(analysis.capabilities[0].value["has_tic_callback"], true);
    assert_eq!(verification.severity_max, "ok");
    assert!(verification.diagnostics.is_empty());
}

#[test]
fn syntactically_invalid_lua_is_rejected_without_execution() {
    let bundle = Tic80Plugin::new();
    let artifact = lua_cart("function TIC( end");
    let verification = bundle.verifier().unwrap().verify_bytes(&artifact).unwrap();

    assert_eq!(verification.severity_max, "error");
    assert_eq!(
        diagnostic(&verification, "lua_compile_error")["level"],
        "error"
    );
    assert_eq!(
        diagnostic(&verification, "missing_tic_callback")["level"],
        "error"
    );
}
