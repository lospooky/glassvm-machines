use std::collections::BTreeSet;

use glassvm_core::{
    ArtifactEncoding, InputApplicationSemantics, InputCatalog, InputDeliveryMode, InputId,
    InputSpec, MachineArtifactSpec, MachineContract, SchemaFamilyId, SchemaRef, SchemaVersion,
};

use crate::identity::{CHIP8_MAX_ROM_BYTES, CHIP8_ROM_START, schema};

pub(super) fn interestingness_schema() -> SchemaRef {
    SchemaRef::new("chip8.interestingness", SchemaVersion::new(6, 0, 0))
}

fn key_input_schema() -> SchemaRef {
    schema("chip8.input.key")
}

fn key_input_application_schema() -> SchemaRef {
    schema("chip8.input.key.application")
}

fn key_input_family() -> SchemaFamilyId {
    SchemaFamilyId::new("glassvm.input.digital-key").expect("canonical CHIP-8 input family")
}

/// The publication contract exposes each hexadecimal key as a distinct typed
/// input. The machine still applies those values to its native 16-bit keypad
/// state internally; the public contract does not expose that native mask.
pub(super) fn chip8_contract() -> MachineContract {
    let payload_schema = key_input_schema();
    let application = InputApplicationSemantics::new(key_input_application_schema())
        .expect("canonical CHIP-8 input application schema");
    let family = key_input_family();
    let delivery_modes = BTreeSet::from([InputDeliveryMode::Scheduled, InputDeliveryMode::Live]);
    let inputs = (0..16)
        .map(|key| InputSpec {
            id: InputId::new(format!("chip8.key.{key:x}")).expect("canonical CHIP-8 key input ID"),
            payload_schema: payload_schema.clone(),
            schema_family: Some(family.clone()),
            coordinate_policy: glassvm_core::CoordinatePolicy::frame_start(),
            delivery_modes: delivery_modes.clone(),
            application: application.clone(),
        })
        .collect();

    MachineContract {
        artifact: MachineArtifactSpec {
            schema: schema("chip8.genome.raw-bytes"),
            encoding: ArtifactEncoding::RawBytes,
            min_bytes: 1,
            max_bytes: CHIP8_MAX_ROM_BYTES,
            alignment_bytes: 1,
            load_address: Some(CHIP8_ROM_START),
        },
        inputs: InputCatalog {
            schema: glassvm_core::input_catalog_schema(),
            inputs,
        },
    }
}
