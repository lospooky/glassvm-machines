//! Hexwell: Human-facing machine descriptor and native-semantics declaration.

use glassvm_core::{
    ExecutionCoordinateCatalog, FrameBoundaryKind, FrameCoordinateDescriptor, MachineDescriptor,
    MachineId, MachineSemantics, StepCoordinateDescriptor, StepUnit, VersionStamp,
    frame_coordinate_schema, step_coordinate_schema,
};

use crate::identity::{
    EMULATOR_VERSION, MACHINE_ID, MACHINE_VERSION, schema, session_snapshot_schema,
};

pub(crate) fn descriptor() -> MachineDescriptor {
    MachineDescriptor {
        id: MachineId::from(MACHINE_ID),
        display_name: "Hexwell Catalyst Plate".into(),
        bundle_version: VersionStamp::from(env!("CARGO_PKG_VERSION")),
        machine_version: VersionStamp::from(MACHINE_VERSION),
        emulator_version: VersionStamp::from(EMULATOR_VERSION),
        variants: vec!["hexwell-v1".into()],
        semantics: MachineSemantics {
            reset: "restore the exact seeded plate, field, reaction front, tide, and evidence state"
                .into(),
            boot: "place fixed matter charges around the center of an immutable 16x16 catalyst plate"
                .into(),
            timing:
                "one cycle is one simultaneous reaction sweep; cycles_per_frame selects sweeps before cooling"
                    .into(),
            memory:
                "immutable 256-byte catalyst lattice over bounded brine, ember, crystal, heat, and spark fields"
                    .into(),
            instruction_schema: schema("hexwell.instruction.catalyst8"),
            state_schema: session_snapshot_schema(),
            native_event_schema: schema("hexwell.event"),
            execution_coordinates: ExecutionCoordinateCatalog {
                frame: FrameCoordinateDescriptor {
                    schema: frame_coordinate_schema(),
                    boundary: FrameBoundaryKind::WorkGrouped,
                },
                step: Some(StepCoordinateDescriptor {
                    schema: step_coordinate_schema(),
                    semantic_unit: StepUnit::BundleDefined(schema("hexwell.coordinate.sweep")),
                }),
            },
        },
    }
}
