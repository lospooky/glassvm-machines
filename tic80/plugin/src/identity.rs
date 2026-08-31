use glassvm_core::{SchemaRef, SchemaVersion};

pub(super) const MACHINE_ID: &str = tic80_core::MACHINE_ID;
pub(super) const SEMANTICS: &str = tic80_core::SEMANTICS;

pub(super) const EMULATOR_VERSION: &str = "tic80_plugin/0.1.0+lua54.input-snapshot-v3";
pub(super) const REPLAY_SNAPSHOT_FORMAT_VERSION: u32 = 3;

pub(super) fn schema(id: &str) -> SchemaRef {
    SchemaRef::new(id, SchemaVersion::V1)
}

pub(super) fn replay_snapshot_schema() -> SchemaRef {
    SchemaRef::new("tic80.lua.replay-snapshot", SchemaVersion::new(3, 0, 0))
}
