use glassvm_core::{SchemaRef, SchemaVersion};

pub const MACHINE_ID: &str = pico8_core::MACHINE_ID;
pub const SEMANTICS: &str = pico8_core::SEMANTICS;
pub const EMULATOR_VERSION: &str = "pico8-headless-lua54";
pub const DEFAULT_INSTRUCTION_BUDGET: u64 = 500_000;
pub const MAX_ARTIFACT_BYTES: usize = 4 * 1024 * 1024;
pub const SESSION_SNAPSHOT_FORMAT: &str = "pico8.session_snapshot";
pub const SESSION_SNAPSHOT_FORMAT_VERSION: u32 = 2;
/// Per-completed-frame count of covered drawing API invocations (schema 1.0.0).
pub const GRAPHICS_ACTIVITY: &str = "glassvm.graphics_activity";
pub const EXECUTION_ACTIVITY: &str = "glassvm.execution_activity";
pub const RNG_ACTIVITY: &str = "glassvm.rng_activity";
/// Per-frame successful calls to PICO-8 `btn` and `btnp` APIs (schema 1.0.0).
pub const INPUT_API_ACTIVITY: &str = "glassvm.input_api_activity";

pub fn schema(id: &str) -> SchemaRef {
    SchemaRef::new(id, SchemaVersion::V1)
}

pub fn execution_activity_schema() -> SchemaRef {
    SchemaRef::new("glassvm.execution_activity", SchemaVersion::new(2, 0, 0))
}

pub fn session_snapshot_schema() -> SchemaRef {
    SchemaRef::new("pico8.state.session_snapshot", SchemaVersion::new(2, 0, 0))
}
