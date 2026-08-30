//! Stable machine, emulator, and schema identities.

use glassvm_core::{SchemaRef, SchemaVersion};

pub const MACHINE_ID: &str = wyrd16_core::MACHINE_ID;
pub(crate) const SEMANTICS: &str = wyrd16_core::SEMANTICS;
pub(crate) const MACHINE_VERSION: &str = SEMANTICS;
pub(crate) const EMULATOR_VERSION: &str = "wyrd16_plugin/0.1.0+clean-contract.snapshot.v5";
pub(crate) const SNAPSHOT_FORMAT: &str = "wyrd16.snapshot";
pub(crate) const SNAPSHOT_VERSION: u16 = 5;

pub(crate) fn schema(id: &str) -> SchemaRef {
    SchemaRef::new(id, SchemaVersion::V1)
}

pub(crate) fn snapshot_schema() -> SchemaRef {
    SchemaRef::new("wyrd16.state.snapshot", SchemaVersion::new(5, 0, 0))
}
