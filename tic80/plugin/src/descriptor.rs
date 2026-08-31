use glassvm_core::{MachineDescriptor, MachineId, MachineSemantics, VersionStamp};

use crate::identity::{EMULATOR_VERSION, MACHINE_ID, SEMANTICS, replay_snapshot_schema, schema};

pub(super) fn descriptor() -> MachineDescriptor {
    MachineDescriptor {
        id: MachineId::from(MACHINE_ID),
        display_name: "TIC-80 (Lua 5.4 compatibility runtime)".into(),
        bundle_version: VersionStamp::from(env!("CARGO_PKG_VERSION")),
        machine_version: VersionStamp::from(SEMANTICS),
        emulator_version: VersionStamp::from(EMULATOR_VERSION),
        variants: vec!["tic80-lua".into()],
        semantics: MachineSemantics {
            reset: "reload cartridge, rebuild Lua VM, run BOOT(), and clear frame/input history".into(),
            boot: "parse .tic chunks, map bank-0 assets into 96 KiB RAM, load source into the bounded Lua 5.4 compatibility runtime, run BOOT()".into(),
            timing: "one TIC() callback per deterministic 60 Hz frame".into(),
            memory: "96 KiB byte-addressed TIC-80 I/O RAM with two 16 KiB VRAM banks".into(),
            instruction_schema: schema("tic80.lua.source"),
            state_schema: replay_snapshot_schema(),
            native_event_schema: schema("tic80.event"),
        },
    }
}
