use std::collections::BTreeMap;
use std::sync::Arc;

use glassvm_core::{
    ExecutionRequest, MachineBundle, MachineId, ObservationPlan, ResolvedEpisodeContext, RunConfig,
    fixed_body_action_schema, fixed_body_identity, no_input_policy_identity,
    null_environment_identity,
};
use glassvm_registry::Registry;
use serde_json::json;
use tic80_core::{MACHINE_ID, SEMANTICS};
use tic80_plugin::Tic80Plugin;

const SMOKE: &[u8] = include_bytes!("../../fixtures/smoke.rom");

fn config() -> RunConfig {
    RunConfig {
        max_frames: 1,
        cycles_per_frame: 1,
        seed: 7,
        machine_params: BTreeMap::new(),
        record_frames: false,
        record_events: false,
    }
}

#[test]
fn bundle_contract_registers_and_uses_the_core_identity_authority() {
    let bundle = Tic80Plugin::new();
    bundle.validate_contract().expect("valid contract");
    assert_eq!(bundle.descriptor().id, MachineId::from(MACHINE_ID));
    assert_eq!(bundle.descriptor().machine_version.0, SEMANTICS);
    assert!(!bundle.contract().default_body.preserves_native_semantics);

    let mut registry = Registry::new();
    registry
        .register(Arc::new(bundle))
        .expect("register TIC-80 bundle");
    assert!(registry.get(&MachineId::from(MACHINE_ID)).is_some());
}

#[test]
fn backend_materializes_the_canonical_episode_and_rejects_a_forged_body() {
    let plugin = Tic80Plugin::new();
    let body = &plugin.contract().default_body;
    let version = plugin.descriptor().bundle_version.clone();
    let expected_body = fixed_body_identity(body, version.clone());

    let request = ExecutionRequest::new(
        "tic80-body-context",
        MACHINE_ID,
        config(),
        ObservationPlan::fitness(),
    )
    .expect("request");
    let session = plugin
        .emulator()
        .create_execution(SMOKE, request)
        .expect("session");
    let episode = session
        .request()
        .episode
        .as_ref()
        .expect("resolved episode");
    assert_eq!(episode.body, expected_body);
    assert_eq!(episode.interaction_policy.id, "glassvm.input.none.v1");

    let provider = plugin
        .body_provider(&body.id)
        .expect("canonical body provider");
    assert_eq!(provider.action_schema(), fixed_body_action_schema(body));
    assert_eq!(
        provider
            .resolve_identity(&body.parameters)
            .expect("body identity"),
        fixed_body_identity(body, version.clone())
    );
    let mut altered = body.parameters.clone();
    altered["width"] = json!(1);
    assert!(provider.resolve_identity(&altered).is_err());

    let mut forged_body = expected_body;
    forged_body.id = "tic80.forged".into();
    let forged = ResolvedEpisodeContext::new(
        no_input_policy_identity(version.clone()),
        forged_body,
        null_environment_identity(version),
        None,
        None,
    )
    .expect("structurally valid forged context");
    let request = ExecutionRequest::new(
        "tic80-forged-context",
        MACHINE_ID,
        config(),
        ObservationPlan::fitness(),
    )
    .expect("request")
    .with_resolved_episode(forged)
    .expect("forged request");
    let error = plugin
        .emulator()
        .create_execution(SMOKE, request)
        .err()
        .expect("forged body must fail");
    assert!(error.contains("resolved body"), "{error}");
}
