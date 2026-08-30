use glassvm_core::{
    ExecutionCoordinateCatalog, FrameBoundaryKind, FrameCoordinateDescriptor, MachineDescriptor,
    MachineId, MachineSemantics, StepCoordinateDescriptor, StepUnit, VersionStamp,
    frame_coordinate_schema, step_coordinate_schema,
};

use crate::identity::{CHIP8_ID, EMULATOR_VERSION, SEMANTICS, schema, session_snapshot_schema};

pub(super) fn chip8_descriptor() -> MachineDescriptor {
    MachineDescriptor {
        id: MachineId::from(CHIP8_ID),
        display_name: "CHIP-8 / SCHIP / XO-CHIP".into(),
        bundle_version: VersionStamp::from(env!("CARGO_PKG_VERSION")),
        machine_version: VersionStamp::from(SEMANTICS),
        emulator_version: VersionStamp::from(EMULATOR_VERSION),
        variants: vec![
            "chip8".into(),
            "chip48".into(),
            "vip".into(),
            "schip".into(),
            "xochip".into(),
        ],
        semantics: MachineSemantics {
            reset: "restore the post-ROM-load CPU snapshot and clear session counters".into(),
            boot: "load raw bytes at address 0x200 with the selected quirk profile".into(),
            timing: "configurable instructions per 60 Hz timer/frame tick".into(),
            memory: "64 KiB byte-addressed memory; program start 0x200".into(),
            instruction_schema: schema("chip8.instruction.word16"),
            state_schema: session_snapshot_schema(),
            native_event_schema: schema("chip8.event"),
            execution_coordinates: ExecutionCoordinateCatalog {
                frame: FrameCoordinateDescriptor {
                    schema: frame_coordinate_schema(),
                    boundary: FrameBoundaryKind::Hardware,
                },
                step: Some(StepCoordinateDescriptor {
                    schema: step_coordinate_schema(),
                    semantic_unit: StepUnit::Instruction,
                }),
            },
        },
    }
}
