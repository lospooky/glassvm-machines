use glassvm_core::{SchemaRef, SchemaVersion};

pub const MACHINE_ID: &str = chip8_core::MACHINE_ID;
pub const SEMANTICS: &str = chip8_core::SEMANTICS;
pub(super) const CHIP8_ID: &str = MACHINE_ID;
pub(super) const EMULATOR_VERSION: &str =
    "chip8_core/0.1.0+glassvm-scheduled-input.v2.visual-ecology.v4";
pub(super) const SESSION_SNAPSHOT_FORMAT: &str = "chip8.session.snapshot";
pub(super) const SESSION_SNAPSHOT_FORMAT_VERSION: u32 = 5;
pub(super) const CHIP8_ROM_START: u64 = 0x200;
pub const CHIP8_MAX_ROM_BYTES: usize = 65_536 - CHIP8_ROM_START as usize;
pub const DISPLAY_FRAME_PAYLOAD_SCHEMA_VERSION: u32 = 2;
pub fn schema(id: &str) -> SchemaRef {
    SchemaRef::new(id, SchemaVersion::V1)
}

pub(super) fn session_snapshot_schema() -> SchemaRef {
    SchemaRef::new(SESSION_SNAPSHOT_FORMAT, SchemaVersion::new(5, 0, 0))
}
