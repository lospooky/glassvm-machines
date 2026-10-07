use std::collections::BTreeSet;

use glassvm_core::{
    ArtifactEncoding, InputApplicationSemantics, InputCatalog, InputDeliveryMode, InputId,
    InputSpec, MachineArtifactSpec, MachineContract, SchemaFamilyId,
};

use crate::identity::{MAX_ARTIFACT_BYTES, schema};

pub(super) fn contract() -> MachineContract {
    let payload_schema = schema("pico8.input.button");
    let application = InputApplicationSemantics::new(schema("pico8.input.button.application"))
        .expect("canonical PICO-8 input application schema");
    let family =
        SchemaFamilyId::new("glassvm.input.digital-key").expect("canonical PICO-8 input family");
    let delivery_modes = BTreeSet::from([InputDeliveryMode::Scheduled]);
    let inputs = (0..16)
        .map(|button| InputSpec {
            id: InputId::new(format!("pico8.button.{button:x}"))
                .expect("canonical PICO-8 button input ID"),
            payload_schema: payload_schema.clone(),
            schema_family: Some(family.clone()),
            coordinate_policy: glassvm_core::CoordinatePolicy::frame_start(),
            delivery_modes: delivery_modes.clone(),
            application: application.clone(),
        })
        .collect();

    MachineContract {
        artifact: MachineArtifactSpec {
            schema: schema("pico8.artifact.cartridge"),
            encoding: ArtifactEncoding::Extension("pico8-cartridge".into()),
            min_bytes: 1,
            max_bytes: MAX_ARTIFACT_BYTES,
            alignment_bytes: 1,
            load_address: None,
        },
        inputs: InputCatalog {
            schema: glassvm_core::input_catalog_schema(),
            inputs,
        },
    }
}
