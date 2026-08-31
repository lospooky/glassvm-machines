use glassvm_core::{
    SchemaRef, SchemaVersion, VersionStamp, VersionedComponentIdentity, fixed_body_identity,
};
use pico8_core::CartridgeFormat;
use serde_json::{Value, json};

use crate::contract::contract;

pub(super) const MACHINE_ID: &str = pico8_core::MACHINE_ID;
pub(super) const PICO8_ID: &str = MACHINE_ID;
pub(super) const SEMANTICS: &str = pico8_core::SEMANTICS;
pub(super) const MACHINE_VERSION: &str = SEMANTICS;
pub(super) const EMULATOR_VERSION: &str = "pico8-to-lua-0.1.1+mlua-0.10.5.v3";
pub(super) const DEFAULT_INSTRUCTION_BUDGET: u64 = 500_000;
pub(super) const MAX_ARTIFACT_BYTES: usize = 4 * 1024 * 1024;
pub(super) const CONTINUATION_SCHEMA_VERSION: u32 = 2;
pub(super) const MAX_CONTINUATION_BYTES: usize = 128 * 1024 * 1024;
pub(super) fn schema(id: &str) -> SchemaRef {
    SchemaRef::new(id, SchemaVersion::V1)
}

pub(super) fn format_name(format: CartridgeFormat) -> &'static str {
    match format {
        CartridgeFormat::P8Text => "p8-text",
        CartridgeFormat::P8Png => "p8-png",
        CartridgeFormat::P8Rom => "p8-rom",
    }
}

pub(super) fn component_identity(
    id: &str,
    schema_id: &str,
    parameters: Value,
) -> VersionedComponentIdentity {
    VersionedComponentIdentity {
        id: id.into(),
        schema: schema(schema_id),
        implementation_version: VersionStamp::from(env!("CARGO_PKG_VERSION")),
        parameters,
    }
}

pub(super) fn pico8_headless_body_identity() -> VersionedComponentIdentity {
    fixed_body_identity(
        &contract().default_body,
        VersionStamp::from(env!("CARGO_PKG_VERSION")),
    )
}

pub(super) fn recorded_controller_policy_identity() -> VersionedComponentIdentity {
    component_identity(
        "pico8.input.recorded_controller_mask.v1",
        "pico8.input.policy",
        json!({"coordinate": "frame_start", "port": "controllers_in"}),
    )
}
