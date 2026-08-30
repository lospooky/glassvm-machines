//! Hexwell: Stable machine, schema, and capability identities.

use glassvm_core::{SchemaRef, SchemaVersion};

pub const MACHINE_ID: &str = hexwell_core::MACHINE_ID;
pub const SEMANTICS: &str = hexwell_core::SEMANTICS;
pub const MACHINE_VERSION: &str = SEMANTICS;
pub(crate) const EMULATOR_VERSION: &str = "hexwell_plugin/0.1.0+clean-contract.snapshot.v4";
pub(crate) const SESSION_SNAPSHOT_FORMAT_VERSION: u16 = 4;

pub(crate) fn schema(id: &str) -> SchemaRef {
    SchemaRef::new(id, SchemaVersion::V1)
}

pub(crate) fn session_snapshot_schema() -> SchemaRef {
    SchemaRef::new(
        "hexwell.state.session-snapshot",
        SchemaVersion::new(SESSION_SNAPSHOT_FORMAT_VERSION, 0, 0),
    )
}
