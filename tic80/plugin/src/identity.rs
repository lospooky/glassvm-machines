use glassvm_core::{SchemaRef, SchemaVersion};

pub const MACHINE_ID: &str = tic80_core::MACHINE_ID;
pub const SEMANTICS: &str = tic80_core::SEMANTICS;
pub const EMULATOR_VERSION: &str = "tic80-lua54";

pub fn schema(id: &str) -> SchemaRef {
    SchemaRef::new(id, SchemaVersion::V1)
}

pub fn session_snapshot_schema() -> SchemaRef {
    schema("tic80.state.session_snapshot")
}
