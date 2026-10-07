use glassvm_core::{
    ExecutionCoordinateCatalog, FrameBoundaryKind, FrameCoordinateDescriptor, MachineDescriptor,
    MachineId, MachineSemantics, StepCoordinateDescriptor, StepUnit, VersionStamp,
    frame_coordinate_schema, step_coordinate_schema,
};

use crate::identity::{EMULATOR_VERSION, MACHINE_ID, SEMANTICS, schema, session_snapshot_schema};

pub(super) fn descriptor() -> MachineDescriptor {
    MachineDescriptor {
        id: MachineId::from(MACHINE_ID),
        display_name: "TIC-80 Lua compatibility runtime".into(),
        bundle_version: VersionStamp::from(env!("CARGO_PKG_VERSION")),
        machine_version: VersionStamp::from(SEMANTICS),
        emulator_version: VersionStamp::from(EMULATOR_VERSION),
        variants: vec!["lua".into()],
        semantics: MachineSemantics {
            reset: "recreate the cartridge runtime from its admitted artifact and configuration"
                .into(),
            boot: "load a Lua TIC-80 cartridge and invoke BOOT when present".into(),
            timing: "one TIC callback boundary per semantic frame".into(),
            memory: "96 KiB RAM with two 16 KiB video banks and a 240x136 display".into(),
            instruction_schema: schema("tic80.instruction.lua_operation"),
            state_schema: session_snapshot_schema(),
            native_event_schema: schema("tic80.native_event"),
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
