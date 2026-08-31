use glassvm_core::{
    BodySpec, FitnessExtensionSpec, GenomeEncoding, GenomeSpec, MachineContract,
    MutationOperatorKind, MutationOperatorSpec, MutationSpec, PortDirection, PortProfile,
    PortSignal, PortSpec,
};
use serde_json::json;

use crate::identity::{MAX_ARTIFACT_BYTES, schema};

pub(super) fn contract() -> MachineContract {
    let ports = vec![
        PortSpec {
            id: "display_out".into(),
            direction: PortDirection::Output,
            signal: PortSignal::Framebuffer,
            native_binding: "0x6000..0x7fff packed 128x128 framebuffer".into(),
            description: "PICO-8 16-colour logical display".into(),
        },
        PortSpec {
            id: "controllers_in".into(),
            direction: PortDirection::Input,
            signal: PortSignal::Digital,
            native_binding: "btn()/btnp() controller bit mask".into(),
            description: "Two six-button logical controllers (16-bit mask envelope)".into(),
        },
        PortSpec {
            id: "audio_out".into(),
            direction: PortDirection::Output,
            signal: PortSignal::Audio,
            native_binding: "sfx()/music() calls and 0x3100..0x42ff cartridge data".into(),
            description: "Headless audio command observations".into(),
        },
        PortSpec {
            id: "rng_in".into(),
            direction: PortDirection::Input,
            signal: PortSignal::Randomness,
            native_binding: "srand()/rnd() deterministic compatibility generator".into(),
            description: "Replay-seeded deterministic randomness".into(),
        },
        PortSpec {
            id: "gpio".into(),
            direction: PortDirection::Bidirectional,
            signal: PortSignal::Numeric,
            native_binding: "0x5f80..0x5fff".into(),
            description: "PICO-8 GPIO memory window".into(),
        },
    ];
    let port_ids = ports.iter().map(|port| port.id.clone()).collect();
    MachineContract {
        genome: GenomeSpec {
            schema: schema("pico8.genome.cartridge"),
            encoding: GenomeEncoding::Extension("pico8.cartridge".into()),
            min_bytes: 1,
            max_bytes: MAX_ARTIFACT_BYTES,
            alignment_bytes: 1,
            load_address: None,
        },
        mutation: MutationSpec {
            schema: schema("pico8.mutation"),
            preserves_raw_byte_baseline: true,
            operators: vec![
                MutationOperatorSpec {
                    id: "byte_substitution".into(),
                    kind: MutationOperatorKind::ByteSubstitution,
                    changes_length: false,
                    description: "Replace one artifact byte".into(),
                },
                MutationOperatorSpec {
                    id: "byte_insertion".into(),
                    kind: MutationOperatorKind::ByteInsertion,
                    changes_length: true,
                    description: "Insert one artifact byte".into(),
                },
                MutationOperatorSpec {
                    id: "byte_deletion".into(),
                    kind: MutationOperatorKind::ByteDeletion,
                    changes_length: true,
                    description: "Delete one artifact byte".into(),
                },
                MutationOperatorSpec {
                    id: "pico8_section_aware".into(),
                    kind: MutationOperatorKind::InstructionAware,
                    changes_length: true,
                    description: "Optional mutation constrained to a decoded Lua or asset section"
                        .into(),
                },
            ],
        },
        ports: PortProfile {
            schema: schema("pico8.ports"),
            ports,
        },
        default_body: BodySpec {
            id: "pico8.headless".into(),
            display_name: "Headless PICO-8 display, controllers, audio events, RNG, and GPIO"
                .into(),
            schema: schema("pico8.body.headless"),
            preserves_native_semantics: false,
            ports: port_ids,
            parameters: json!({
                "compatibility_tier": "documented-headless-subset",
                "lua_engine": "Lua 5.4 via mlua",
                "numeric_model": "host floating point; not PICO-8 16.16 exact",
                "audio": "command observation; no waveform synthesis"
            }),
        },
        alternate_bodies: Vec::new(),
        fitness_extensions: vec![FitnessExtensionSpec {
            id: "pico8.execution_summary".into(),
            description: "Callback, draw, audio, print, source, and final-frame summary".into(),
            output_schema: schema("pico8.execution_summary"),
        }],
    }
}
