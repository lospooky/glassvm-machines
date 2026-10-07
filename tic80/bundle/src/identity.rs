use glassvm_core::{SchemaRef, SchemaVersion};

pub const MACHINE_ID: &str = tic80_core::MACHINE_ID;
pub const SEMANTICS: &str = tic80_core::SEMANTICS;
pub const EMULATOR_VERSION: &str = "tic80-lua54";
pub const SESSION_SNAPSHOT_FORMAT: &str = "tic80.session_snapshot";
pub const SESSION_SNAPSHOT_FORMAT_VERSION: u32 = 2;

pub fn schema(id: &str) -> SchemaRef {
    SchemaRef::new(id, SchemaVersion::V1)
}

pub fn session_snapshot_schema() -> SchemaRef {
    SchemaRef::new("tic80.state.session_snapshot", SchemaVersion::new(2, 0, 0))
}
