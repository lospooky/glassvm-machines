use std::collections::BTreeSet;

use glassvm_core::{
    ArtifactEncoding, InputApplicationSemantics, InputCatalog, InputDeliveryMode, InputId,
    InputSpec, MachineArtifactSpec, MachineContract, SchemaRef,
};

use crate::identity::schema;
use hexwell_core::WELL_COUNT;

fn tide_input_schema() -> SchemaRef {
    schema("hexwell.input.tide")
}

fn tide_input_application_schema() -> SchemaRef {
    schema("hexwell.input.tide.application")
}

/// Hexwell exposes its frame-start tide control as one typed eight-bit input.
/// Portal, materia, and ignition bits remain the machine-specific payload
/// semantics behind this declared input; they are not a generic actuation DSL.
pub(crate) fn contract() -> MachineContract {
    MachineContract {
        artifact: MachineArtifactSpec {
            schema: schema("hexwell.artifact.catalyst-plate"),
            encoding: ArtifactEncoding::RawBytes,
            min_bytes: WELL_COUNT,
            max_bytes: WELL_COUNT,
            alignment_bytes: 1,
            load_address: None,
        },
        inputs: InputCatalog {
            schema: glassvm_core::input_catalog_schema(),
            inputs: vec![InputSpec {
                id: InputId::new("hexwell.tide").expect("canonical Hexwell tide input ID"),
                payload_schema: tide_input_schema(),
                schema_family: None,
                coordinate_policy: glassvm_core::CoordinatePolicy::frame_start(),
                delivery_modes: BTreeSet::from([InputDeliveryMode::Scheduled]),
                application: InputApplicationSemantics::new(tide_input_application_schema())
                    .expect("canonical Hexwell tide application schema"),
            }],
        },
    }
}
