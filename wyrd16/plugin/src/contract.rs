use std::collections::BTreeSet;

use glassvm_core::{
    ArtifactEncoding, InputApplicationSemantics, InputCatalog, InputDeliveryMode, InputId,
    InputSpec, MachineArtifactSpec, MachineContract, SchemaFamilyId, SchemaRef,
};

use crate::{MAX_ROM_BYTES, identity::schema};

fn key_input_schema() -> SchemaRef {
    schema("wyrd16.input.key")
}

fn key_input_application_schema() -> SchemaRef {
    schema("wyrd16.input.key.application")
}

fn key_input_family() -> SchemaFamilyId {
    SchemaFamilyId::new("glassvm.input.digital-key").expect("canonical Wyrd-16 input family")
}

/// Wyrd-16 exposes eight semantic boolean key inputs. The native machine
/// still consumes an eight-bit mask internally; that representation is not
/// part of the publication-facing contract.
pub(crate) fn contract() -> MachineContract {
    let payload_schema = key_input_schema();
    let application = InputApplicationSemantics::new(key_input_application_schema())
        .expect("canonical Wyrd-16 input application schema");
    let family = key_input_family();
    let delivery_modes = BTreeSet::from([InputDeliveryMode::Scheduled]);
    let inputs = (0..8)
        .map(|key| InputSpec {
            id: InputId::new(format!("wyrd16.key.{key}")).expect("canonical Wyrd-16 key input ID"),
            payload_schema: payload_schema.clone(),
            schema_family: Some(family.clone()),
            coordinate_policy: glassvm_core::CoordinatePolicy::frame_start(),
            delivery_modes: delivery_modes.clone(),
            application: application.clone(),
        })
        .collect();

    MachineContract {
        artifact: MachineArtifactSpec {
            schema: schema("wyrd16.artifact.raw-runes"),
            encoding: ArtifactEncoding::RawBytes,
            min_bytes: 2,
            max_bytes: MAX_ROM_BYTES,
            alignment_bytes: 2,
            load_address: Some(0),
        },
        inputs: InputCatalog {
            schema: glassvm_core::input_catalog_schema(),
            inputs,
        },
    }
}
