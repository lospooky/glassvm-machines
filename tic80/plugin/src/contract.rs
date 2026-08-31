use std::collections::BTreeSet;

use glassvm_core::{
    ArtifactEncoding, InputApplicationSemantics, InputCatalog, InputDeliveryMode, InputId,
    InputSpec, MachineArtifactSpec, MachineContract,
};

use crate::identity::schema;

pub(super) fn contract() -> MachineContract {
    MachineContract {
        artifact: MachineArtifactSpec {
            schema: schema("tic80.artifact.cartridge"),
            encoding: ArtifactEncoding::Extension("tic80-cartridge".into()),
            min_bytes: 1,
            max_bytes: tic80_core::MAX_CART_BYTES,
            alignment_bytes: 1,
            load_address: None,
        },
        inputs: InputCatalog {
            schema: glassvm_core::input_catalog_schema(),
            inputs: vec![InputSpec {
                id: InputId::new("tic80.gamepad").expect("canonical TIC-80 input ID"),
                payload_schema: schema("tic80.input.gamepad"),
                schema_family: None,
                coordinate_policy: glassvm_core::CoordinatePolicy::frame_start(),
                delivery_modes: BTreeSet::from([InputDeliveryMode::Scheduled]),
                application: InputApplicationSemantics::new(schema(
                    "tic80.input.gamepad.application",
                ))
                .expect("canonical TIC-80 input application schema"),
            }],
        },
    }
}
