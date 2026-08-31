use glassvm_core::{
    EventContext, MachineBundle, MachineId, NativeEvent, NativeObservation, SchemaRef,
    SchemaVersion, VersionStamp,
};
use pico8_plugin::Pico8Plugin;
use serde_json::json;

#[test]
fn native_framebuffer_is_normalized_to_the_shared_display_capability() {
    let plugin = Pico8Plugin::new();
    let payload = json!({
        "width": 128,
        "height": 128,
        "planes": 1,
        "bytes": [0, 1, 2, 3]
    });

    let capabilities = plugin
        .observable_adapter()
        .normalize(&[NativeObservation {
            schema: SchemaRef::new("pico8.framebuffer", SchemaVersion::V1),
            name: "pico8.framebuffer_flat".into(),
            payload: payload.clone(),
        }])
        .expect("normalize framebuffer");

    assert_eq!(capabilities.len(), 1);
    assert_eq!(capabilities[0].key, "display.framebuffer_flat");
    assert_eq!(capabilities[0].payload, payload);
}

#[test]
fn public_adapters_reject_foreign_context_schema_kind_and_payload_shapes() {
    let plugin = Pico8Plugin::new();
    let context = EventContext {
        arch: plugin.descriptor().id.clone(),
        machine_version: plugin.descriptor().machine_version.clone(),
        run_id: "pico8-adapter-hardening".into(),
        sequence: 0,
        step: 0,
        cycle_or_tick: Some(0),
        frame: Some(0),
        pc: None,
        instruction: None,
    };
    let native = NativeEvent {
        schema: SchemaRef::new("pico8.event", SchemaVersion::V1),
        kind: "display_write".into(),
        payload: json!({"x": 7, "y": 9, "color": 11}),
    };
    let adapter = plugin.native_event_adapter();
    adapter
        .normalize(&context, &native)
        .expect("canonical event");
    let mut foreign_arch = context.clone();
    foreign_arch.arch = MachineId::from("foreign");
    assert!(adapter.normalize(&foreign_arch, &native).is_err());
    let mut foreign_version = context.clone();
    foreign_version.machine_version = VersionStamp::from("foreign-v1");
    assert!(adapter.normalize(&foreign_version, &native).is_err());
    let mut wrong_schema = native.clone();
    wrong_schema.schema.version = SchemaVersion::new(2, 0, 0);
    assert!(adapter.normalize(&context, &wrong_schema).is_err());
    let mut unknown = native.clone();
    unknown.kind = "oracle".into();
    assert!(adapter.normalize(&context, &unknown).is_err());
    let mut malformed = native.clone();
    malformed.payload = json!({"x": 7, "y": 9, "color": "red"});
    assert!(adapter.normalize(&context, &malformed).is_err());
    let mut extra = native;
    extra.payload["extra"] = json!(true);
    assert!(adapter.normalize(&context, &extra).is_err());

    let observation = NativeObservation {
        schema: SchemaRef::new("pico8.framebuffer", SchemaVersion::V1),
        name: "pico8.framebuffer_flat".into(),
        payload: json!({"width": 128, "height": 128, "planes": 1, "bytes": [0]}),
    };
    let observable = plugin.observable_adapter();
    observable
        .normalize(std::slice::from_ref(&observation))
        .expect("canonical observation");
    let mut wrong_schema = observation.clone();
    wrong_schema.schema.version = SchemaVersion::new(2, 0, 0);
    assert!(observable.normalize(&[wrong_schema]).is_err());
    let mut wrong_schema_name = observation.clone();
    wrong_schema_name.schema.id = "pico8.display_dims".into();
    assert!(observable.normalize(&[wrong_schema_name]).is_err());
    let mut unknown = observation.clone();
    unknown.name = "pico8.unknown".into();
    assert!(observable.normalize(&[unknown]).is_err());
    let mut malformed = observation.clone();
    malformed.payload["bytes"] = json!([256]);
    assert!(observable.normalize(&[malformed]).is_err());
    let mut extra = observation;
    extra.payload["extra"] = json!(true);
    assert!(observable.normalize(&[extra]).is_err());
}
