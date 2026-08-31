use glassvm_core::{SchemaRef, SchemaVersion};

pub const MACHINE_ID: &str = pico8_core::MACHINE_ID;
pub const SEMANTICS: &str = pico8_core::SEMANTICS;
pub const EMULATOR_VERSION: &str = "pico8-headless-lua54";
pub const DEFAULT_INSTRUCTION_BUDGET: u64 = 500_000;
pub const MAX_ARTIFACT_BYTES: usize = 4 * 1024 * 1024;

pub fn schema(id: &str) -> SchemaRef {
    SchemaRef::new(id, SchemaVersion::V1)
}

pub fn session_snapshot_schema() -> SchemaRef {
    schema("pico8.state.session_snapshot")
}
