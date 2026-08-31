use glassvm_core::{
    BodySpec, FitnessExtensionSpec, GenomeEncoding, GenomeSpec, MachineContract,
    MutationOperatorKind, MutationOperatorSpec, MutationSpec, PortDirection, PortProfile,
    PortSignal, PortSpec,
};
use serde_json::json;
use tic80_core::{HEIGHT, MAX_CART_BYTES, WIDTH};

use crate::identity::schema;

pub(super) fn contract() -> MachineContract {
    let ports = vec![
        PortSpec {
            id: "display_out".into(),
            direction: PortDirection::Output,
            signal: PortSignal::Framebuffer,
            native_binding: "240x136 4-bpp VRAM and active 16-color palette".into(),
            description: "TIC-80 display output".into(),
        },
        PortSpec {
            id: "gamepad_in".into(),
            direction: PortDirection::Input,
            signal: PortSignal::Digital,
            native_binding: "four 8-button gamepads at RAM 0x0ff80".into(),
            description: "Packed native TIC-80 gamepad state".into(),
        },
        PortSpec {
            id: "sound_out".into(),
            direction: PortDirection::Output,
            signal: PortSignal::Audio,
            native_binding: "four TIC-80 sound channels".into(),
            description: "Declared native sound port; synthesis evidence is not yet emitted".into(),
        },
        PortSpec {
            id: "trace_out".into(),
            direction: PortDirection::Output,
            signal: PortSignal::Event,
            native_binding: "trace() callback".into(),
            description: "Cartridge diagnostic trace output".into(),
        },
        PortSpec {
            id: "rng_in".into(),
            direction: PortDirection::Input,
            signal: PortSignal::Randomness,
            native_binding: "Lua math.random seed".into(),
            description: "Deterministic runtime seed".into(),
        },
    ];
    MachineContract {
        genome: GenomeSpec {
            schema: schema("tic80.cartridge.binary"),
            encoding: GenomeEncoding::Extension(".tic".into()),
            min_bytes: 5,
            max_bytes: MAX_CART_BYTES,
            alignment_bytes: 1,
            load_address: None,
        },
        mutation: MutationSpec {
            schema: schema("tic80.mutation"),
            preserves_raw_byte_baseline: true,
            operators: vec![
                MutationOperatorSpec {
                    id: "byte_substitution".into(),
                    kind: MutationOperatorKind::ByteSubstitution,
                    changes_length: false,
                    description: "Replace one raw cartridge byte".into(),
                },
                MutationOperatorSpec {
                    id: "bit_flip".into(),
                    kind: MutationOperatorKind::BitFlip,
                    changes_length: false,
                    description: "Flip one raw cartridge bit".into(),
                },
                MutationOperatorSpec {
                    id: "chunk_aware".into(),
                    kind: MutationOperatorKind::InstructionAware,
                    changes_length: true,
                    description: "Mutate a parsed TIC-80 chunk while preserving its header".into(),
                },
                MutationOperatorSpec {
                    id: "repair_chunk_sizes".into(),
                    kind: MutationOperatorKind::Repair,
                    changes_length: false,
                    description: "Repair little-endian .tic chunk length fields".into(),
                },
            ],
        },
        ports: PortProfile {
            schema: schema("tic80.ports"),
            ports,
        },
        default_body: BodySpec {
            id: "tic80.native".into(),
            display_name: "TIC-80 Lua compatibility display and four gamepads".into(),
            schema: schema("tic80.body.native"),
            preserves_native_semantics: false,
            ports: vec![
                "display_out".into(),
                "gamepad_in".into(),
                "sound_out".into(),
                "trace_out".into(),
                "rng_in".into(),
            ],
            parameters: json!({
                "compatibility_tier": "bounded-lua54-headless-subset",
                "width": WIDTH,
                "height": HEIGHT,
                "fps": 60
            }),
        },
        alternate_bodies: Vec::new(),
        fitness_extensions: vec![FitnessExtensionSpec {
            id: "tic80.frame_summary".into(),
            description: "TIC-80 framebuffer and trace summary".into(),
            output_schema: schema("tic80.frame-summary"),
        }],
    }
}
