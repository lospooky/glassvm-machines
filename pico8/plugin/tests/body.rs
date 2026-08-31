use glassvm_core::{BodyAction, MachineBundle, TemporalCoordinate};
use pico8_plugin::Pico8Plugin;
use serde_json::json;

#[test]
fn headless_body_maps_named_buttons_to_controller_bits() {
    let plugin = Pico8Plugin::new();
    let descriptor = &plugin.contract().default_body;
    let provider = plugin
        .body_provider(&descriptor.id)
        .expect("default PICO-8 body provider");
    let mut body = provider
        .create(&descriptor.parameters)
        .expect("headless body runtime");

    let stimuli = body
        .resolve_action(&BodyAction {
            coordinate: TemporalCoordinate::frame_start(0),
            action: "press_button".into(),
            value: json!({"player": 0, "button": "a"}),
        })
        .expect("translate button action");

    assert_eq!(stimuli.len(), 1);
    assert_eq!(stimuli[0].port, "controllers_in");
    assert_eq!(stimuli[0].value, json!({"mask": 16}));
}
