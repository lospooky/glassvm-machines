use glassvm_core::{
    EventContext, EventKind, MachineBundle, MachineId, NativeEvent, NativeObservation, RunId,
    SchemaRef, SchemaVersion, VersionStamp,
};
use serde_json::json;
use tic80_core::{MACHINE_ID, SEMANTICS};
use tic80_plugin::Tic80Plugin;

const SMOKE: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn native_event_and_framebuffer_adapters_preserve_namespaced_payloads() {
    let plugin = Tic80Plugin::new();
    let context = EventContext {
        arch: MachineId::from(MACHINE_ID),
        machine_version: VersionStamp::from(SEMANTICS),
        run_id: RunId::from("tic80-adapter-contract"),
        sequence: 9,
        step: 4,
        cycle_or_tick: Some(4),
        frame: Some(4),
        pc: None,
        instruction: None,
    };
    let native = NativeEvent {
        schema: SchemaRef::new("tic80.event", SchemaVersion::V1),
        kind: "trace".into(),
        payload: json!({"message": "ready"}),
    };
    let events = plugin
        .native_event_adapter()
        .normalize(&context, &native)
        .expect("normalize trace");
    assert_eq!(events[0].kind, EventKind::Extension("tic80.trace".into()));
    assert_eq!(events[0].sequence, context.sequence);
    assert_eq!(events[0].extensions["tic80.trace"], native.payload);

    let payload = json!({
        "width": 240,
        "height": 136,
        "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
        "bytes": null,
    });
    let capabilities = plugin
        .observable_adapter()
        .normalize(&[NativeObservation {
            schema: SchemaRef::new("tic80.framebuffer", SchemaVersion::V1),
            name: "framebuffer_rgba".into(),
            payload: payload.clone(),
        }])
        .expect("normalize framebuffer");
    assert_eq!(capabilities[0].key, "tic80.framebuffer_rgba");
    assert_eq!(capabilities[0].payload, payload);
}

#[test]
fn static_analyzer_and_verifier_adapters_accept_the_real_cartridge() {
    let plugin = Tic80Plugin::new();
    let analysis = plugin
        .static_analyzer()
        .expect("analyzer")
        .analyze_bytes(SMOKE)
        .expect("analyze real cartridge");
    assert_eq!(analysis.capabilities[0].payload["language"], "lua");
    assert_eq!(analysis.capabilities[0].payload["has_tic_callback"], true);

    let verification = plugin
        .verifier()
        .expect("verifier")
        .verify_bytes(SMOKE)
        .expect("verify real cartridge");
    assert_eq!(verification.severity_max, "ok");
    assert!(verification.diagnostics.is_empty());
}

#[test]
fn adapters_reject_foreign_identity_versions_names_and_malformed_payloads() {
    let plugin = Tic80Plugin::new();
    let context = EventContext {
        arch: MachineId::from(MACHINE_ID),
        machine_version: VersionStamp::from(SEMANTICS),
        run_id: RunId::from("tic80-adversarial-adapter"),
        sequence: 0,
        step: 0,
        cycle_or_tick: Some(0),
        frame: Some(0),
        pc: None,
        instruction: None,
    };
    let event = NativeEvent {
        schema: SchemaRef::new("tic80.event", SchemaVersion::V1),
        kind: "input".into(),
        payload: json!({"mask": 1}),
    };
    assert!(
        plugin
            .native_event_adapter()
            .normalize(&context, &event)
            .is_ok()
    );

    let mut foreign_arch = context.clone();
    foreign_arch.arch = MachineId::from("foreign");
    assert!(
        plugin
            .native_event_adapter()
            .normalize(&foreign_arch, &event)
            .is_err()
    );
    let mut foreign_version = context.clone();
    foreign_version.machine_version = VersionStamp::from("tic80-semantics.foreign");
    assert!(
        plugin
            .native_event_adapter()
            .normalize(&foreign_version, &event)
            .is_err()
    );
    for invalid in [
        NativeEvent {
            schema: SchemaRef::new("tic80.event", SchemaVersion::new(2, 0, 0)),
            ..event.clone()
        },
        NativeEvent {
            kind: "input_near_match".into(),
            ..event.clone()
        },
        NativeEvent {
            payload: json!({"mask": 1, "extra": true}),
            ..event.clone()
        },
        NativeEvent {
            payload: json!({"mask": -1}),
            ..event.clone()
        },
    ] {
        assert!(
            plugin
                .native_event_adapter()
                .normalize(&context, &invalid)
                .is_err()
        );
    }

    let payload = json!({
        "width": 240,
        "height": 136,
        "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
        "bytes": null,
    });
    for invalid in [
        NativeObservation {
            schema: SchemaRef::new("tic80.framebuffer", SchemaVersion::new(2, 0, 0)),
            name: "framebuffer_rgba".into(),
            payload: payload.clone(),
        },
        NativeObservation {
            schema: SchemaRef::new("tic80.framebuffer", SchemaVersion::V1),
            name: "framebuffer_rgba_near_match".into(),
            payload: payload.clone(),
        },
        NativeObservation {
            schema: SchemaRef::new("tic80.framebuffer", SchemaVersion::V1),
            name: "framebuffer_rgba".into(),
            payload: json!({"width": 240, "height": 136, "sha256": "bad", "bytes": null, "extra": true}),
        },
    ] {
        assert!(plugin.observable_adapter().normalize(&[invalid]).is_err());
    }
}
