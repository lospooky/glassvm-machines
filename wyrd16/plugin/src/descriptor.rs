//! GlassVM machine descriptor construction.

use glassvm_core::{
    ExecutionCoordinateCatalog, FrameBoundaryKind, FrameCoordinateDescriptor, MachineDescriptor,
    MachineId, MachineSemantics, StepCoordinateDescriptor, StepUnit, VersionStamp,
    frame_coordinate_schema, step_coordinate_schema,
};

use crate::identity::{EMULATOR_VERSION, MACHINE_ID, MACHINE_VERSION, schema, snapshot_schema};

pub(crate) fn descriptor() -> MachineDescriptor {
    MachineDescriptor {
        id: MachineId::from(MACHINE_ID),
        display_name: "Wyrd-16 Rune Computer".into(),
        bundle_version: VersionStamp::from(env!("CARGO_PKG_VERSION")),
        machine_version: VersionStamp::from(MACHINE_VERSION),
        emulator_version: VersionStamp::from(EMULATOR_VERSION),
        variants: vec!["wyrd16-v1".into()],
        semantics: MachineSemantics {
            reset: "restore the exact post-load state, including canvas, palette, and entropy"
                .into(),
            boot: "zero-fill the 4 KiB arena, copy an aligned ROM at byte 0, and begin at PC 0"
                .into(),
            timing: "one rune per cycle; cycles_per_frame runes form one visual frame".into(),
            memory: "4 KiB mutable circular byte arena; aligned 12-bit byte PC".into(),
            instruction_schema: schema("wyrd16.instruction.rune16"),
            state_schema: snapshot_schema(),
            native_event_schema: schema("wyrd16.event"),
            execution_coordinates: ExecutionCoordinateCatalog {
                frame: FrameCoordinateDescriptor {
                    schema: frame_coordinate_schema(),
                    boundary: FrameBoundaryKind::WorkGrouped,
                },
                step: Some(StepCoordinateDescriptor {
                    schema: step_coordinate_schema(),
                    semantic_unit: StepUnit::Action,
                }),
            },
        },
    }
}
