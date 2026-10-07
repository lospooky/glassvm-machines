use glassvm_core::{
    ExecutionCoordinateCatalog, FrameBoundaryKind, FrameCoordinateDescriptor, MachineDescriptor,
    MachineId, MachineSemantics, StepCoordinateDescriptor, StepUnit, VersionStamp,
    frame_coordinate_schema, step_coordinate_schema,
};

use crate::identity::{EMULATOR_VERSION, MACHINE_ID, SEMANTICS, schema, session_snapshot_schema};

pub(super) fn descriptor() -> MachineDescriptor {
    MachineDescriptor {
        id: MachineId::from(MACHINE_ID),
        display_name: "PICO-8 headless compatibility runtime".into(),
        bundle_version: VersionStamp::from(env!("CARGO_PKG_VERSION")),
        machine_version: VersionStamp::from(SEMANTICS),
        emulator_version: VersionStamp::from(EMULATOR_VERSION),
        variants: vec!["p8-text".into(), "p8-png".into(), "p8-rom".into()],
        semantics: MachineSemantics {
            reset: "recreate the cartridge runtime from its admitted artifact and configuration"
                .into(),
            boot: "load a PICO-8 cartridge into the bounded Lua compatibility runtime".into(),
            timing: "one callback boundary per semantic frame".into(),
            memory: "64 KiB PICO-8 memory map with a 128x128 display".into(),
            instruction_schema: schema("pico8.instruction.lua_operation"),
            state_schema: session_snapshot_schema(),
            native_event_schema: schema("pico8.native_event"),
            execution_coordinates: ExecutionCoordinateCatalog {
                frame: FrameCoordinateDescriptor {
                    schema: frame_coordinate_schema(),
                    boundary: FrameBoundaryKind::Callback,
                },
                step: Some(StepCoordinateDescriptor {
                    schema: step_coordinate_schema(),
                    semantic_unit: StepUnit::Tick,
                }),
            },
        },
    }
}
