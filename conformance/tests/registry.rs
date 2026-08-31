use std::sync::Arc;

use chip8_plugin::Chip8Plugin;
use glassvm_core::MachineId;
use glassvm_registry::Registry;
use hexwell_plugin::HexwellPlugin;
use pico8_plugin::Pico8Plugin;
use tic80_plugin::Tic80Plugin;
use wyrd16_plugin::Wyrd16Plugin;

fn register_reference_bundles() -> Registry {
    let mut registry = Registry::new();
    registry
        .register(Arc::new(Chip8Plugin::new()))
        .expect("valid CHIP-8 bundle");
    registry
        .register(Arc::new(HexwellPlugin::new()))
        .expect("valid Hexwell bundle");
    registry
        .register(Arc::new(Wyrd16Plugin::new()))
        .expect("valid Wyrd-16 bundle");
    registry
}

#[test]
fn registry_denominator_is_the_three_publication_bundles() {
    let registry = register_reference_bundles();

    assert_eq!(
        registry.list(),
        vec![
            MachineId::from("chip8"),
            MachineId::from("hexwell"),
            MachineId::from("wyrd16"),
        ]
    );
}

#[test]
fn publication_bundles_have_slim_contracts_and_canonical_catalogs() {
    let registry = register_reference_bundles();

    for machine_id in registry.list() {
        let bundle = registry.get(&machine_id).expect("registered bundle");
        bundle
            .validate_contract()
            .unwrap_or_else(|errors| panic!("{machine_id}: invalid contract: {errors:?}"));
        assert!(
            !bundle.contract().artifact.schema.id.is_empty(),
            "{machine_id}: artifact schema is empty"
        );
        assert!(
            bundle.contract().inputs.validate().is_ok(),
            "{machine_id}: invalid input catalog"
        );
        assert!(
            bundle.normalizer_catalog().validate().is_ok(),
            "{machine_id}: invalid normalizer catalog"
        );
    }
}

#[test]
fn registry_rejects_duplicate_publication_bundle_ids() {
    let mut registry = Registry::new();
    registry
        .register(Arc::new(Chip8Plugin::new()))
        .expect("first registration");
    let error = registry
        .register(Arc::new(Chip8Plugin::new()))
        .expect_err("duplicate registration must fail");

    assert!(error.contains("machine already registered: chip8"));
}

fn register_all_clean_core_bundles() -> Registry {
    let mut registry = register_reference_bundles();
    registry
        .register(Arc::new(Pico8Plugin::new()))
        .expect("valid PICO-8 bundle");
    registry
        .register(Arc::new(Tic80Plugin::new()))
        .expect("valid TIC-80 bundle");
    registry
}

#[test]
fn all_clean_core_bundles_have_slim_contracts_and_canonical_catalogs() {
    let registry = register_all_clean_core_bundles();

    assert_eq!(
        registry.list(),
        vec![
            MachineId::from("chip8"),
            MachineId::from("hexwell"),
            MachineId::from("pico8"),
            MachineId::from("tic80"),
            MachineId::from("wyrd16"),
        ]
    );
    for machine_id in registry.list() {
        let bundle = registry.get(&machine_id).expect("registered bundle");
        bundle
            .validate_contract()
            .unwrap_or_else(|errors| panic!("{machine_id}: invalid contract: {errors:?}"));
        assert!(
            bundle.contract().inputs.validate().is_ok(),
            "{machine_id}: invalid input catalog"
        );
        assert!(
            bundle.normalizer_catalog().validate().is_ok(),
            "{machine_id}: invalid normalizer catalog"
        );
    }
}
