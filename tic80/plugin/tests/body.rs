use glassvm_core::{BodyAction, MachineBundle, TemporalCoordinate};
use serde_json::{Value, json};
use tic80_plugin::Tic80Plugin;

#[test]
fn native_body_maps_stateful_four_player_actions_strictly() {
    let plugin = Tic80Plugin::new();
    let descriptor = &plugin.contract().default_body;
    let provider = plugin
        .body_provider(&descriptor.id)
        .expect("default body provider");
    let mut body = provider
        .create(&descriptor.parameters)
        .expect("fresh body runtime");

    let set = body
        .resolve_action(&BodyAction {
            coordinate: TemporalCoordinate::frame_start(1),
            action: "set_controller_mask".into(),
            value: json!({"mask": 1}),
        })
        .expect("set mask");
    assert_eq!(set[0].value, json!({"mask": 1}));

    let press = body
        .resolve_action(&BodyAction {
            coordinate: TemporalCoordinate::frame_start(1),
            action: "press_button".into(),
            value: json!({"player": 2, "button": "x"}),
        })
        .expect("press player two X");
    assert_eq!(press[0].port, "gamepad_in");
    assert_eq!(press[0].value, json!({"mask": 1_u32 | (1_u32 << 22)}));

    let release = body
        .resolve_action(&BodyAction {
            coordinate: TemporalCoordinate::frame_start(2),
            action: "release_button".into(),
            value: json!({"player": 0, "button": "up"}),
        })
        .expect("release player zero Up");
    assert_eq!(release[0].value, json!({"mask": 1_u32 << 22}));

    let release_all = body
        .resolve_action(&BodyAction {
            coordinate: TemporalCoordinate::frame_start(2),
            action: "release_all".into(),
            value: Value::Null,
        })
        .expect("release all");
    assert_eq!(release_all[0].value, json!({"mask": 0}));

    for invalid in [
        BodyAction {
            coordinate: TemporalCoordinate::frame_start(0),
            action: "set_controller_mask".into(),
            value: json!({"mask": u64::from(u32::MAX) + 1}),
        },
        BodyAction {
            coordinate: TemporalCoordinate::frame_start(0),
            action: "press_button".into(),
            value: json!({"player": 4, "button": "a"}),
        },
        BodyAction {
            coordinate: TemporalCoordinate::frame_start(0),
            action: "press_button".into(),
            value: json!({"player": 0, "button": "start"}),
        },
        BodyAction {
            coordinate: TemporalCoordinate::frame_start(0),
            action: "press_button".into(),
            value: json!({"player": 0, "button": "a", "extra": true}),
        },
        BodyAction {
            coordinate: TemporalCoordinate {
                step: Some(0),
                cycle_or_tick: None,
                frame: Some(0),
            },
            action: "release_all".into(),
            value: Value::Null,
        },
    ] {
        let mut runtime = provider
            .create(&descriptor.parameters)
            .expect("strictness runtime");
        assert!(runtime.resolve_action(&invalid).is_err(), "{invalid:?}");
    }
}
