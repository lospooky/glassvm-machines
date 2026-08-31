use glassvm_core::{MachineDescriptor, MachineId, MachineSemantics, VersionStamp};

use crate::identity::{EMULATOR_VERSION, MACHINE_VERSION, PICO8_ID, schema};

pub(super) fn descriptor() -> MachineDescriptor {
    MachineDescriptor {
        id: MachineId::from(PICO8_ID),
        display_name: "PICO-8 (headless compatibility runtime)".into(),
        bundle_version: VersionStamp::from(env!("CARGO_PKG_VERSION")),
        machine_version: VersionStamp::from(MACHINE_VERSION),
        emulator_version: VersionStamp::from(EMULATOR_VERSION),
        variants: vec![
            "p8-text".into(),
            "p8-png".into(),
            "p8-rom".into(),
        ],
        semantics: MachineSemantics {
            reset: "re-decode the cartridge, rebuild the sandboxed Lua state, and call _init"
                .into(),
            boot: "decode .p8/.p8.png/.p8.rom, materialize cartridge RAM, install the documented headless API subset, load translated PICO-8 Lua, then call _init".into(),
            timing: "_update60 or _update followed by _draw; fixed 60 Hz or 30 Hz callback cadence with a deterministic per-frame VM-instruction budget".into(),
            memory: "64 KiB byte-addressed PICO-8 RAM with documented cartridge, draw-state, hardware, GPIO, and 128x128 packed screen regions".into(),
            instruction_schema: schema("pico8.lua.callback"),
            state_schema: schema("pico8.state.continuation"),
            native_event_schema: schema("pico8.event"),
        },
    }
}
