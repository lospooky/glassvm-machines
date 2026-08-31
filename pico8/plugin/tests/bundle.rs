use std::{collections::BTreeMap, fs};

use glassvm_core::{
    BodyAction, EventContext, EventKind, ExecutionRequest, MachineBundle, MachineId, NativeEvent,
    NativeObservation, ObservationPlan, RunConfig, RunId, SchemaRef, SchemaVersion,
    SnapshotCapture, TemporalCoordinate, TraceCollector, VersionStamp,
};
use glassvm_episode::{EpisodeRequest, InteractionPlan, execute, resolve};
use pico8_core::{Cartridge, CartridgeFormat};
use pico8_plugin::Pico8Plugin;
use serde_json::json;

const SMOKE_ROM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/smoke.rom");

fn config() -> RunConfig {
    RunConfig {
        max_frames: 2,
        cycles_per_frame: 100,
        seed: 17,
        machine_params: BTreeMap::new(),
        record_frames: false,
        record_events: false,
    }
}

#[test]
fn default_body_is_executable_fresh_strict_and_bound_to_sessions() {
    let artifact = fs::read(SMOKE_ROM).expect("read checked-in PICO-8 cartridge");
    let plugin = Pico8Plugin::new();
    let descriptor = &plugin.contract().default_body;
    let provider = plugin
        .body_provider(&descriptor.id)
        .expect("default body provider");

    assert_eq!(provider.descriptor(), descriptor);
    assert_eq!(provider.action_schema().id, "pico8.body.headless.action");
    let identity = provider
        .resolve_identity(&descriptor.parameters)
        .expect("default body identity");
    assert_eq!(identity.id, descriptor.id);
    assert_eq!(identity.schema, descriptor.schema);
    assert_eq!(identity.parameters["ports"], json!(descriptor.ports));
    assert_eq!(
        identity.parameters["body_parameters"],
        descriptor.parameters
    );

    let mut unsupported = descriptor.parameters.clone();
    unsupported["unexpected"] = json!(true);
    assert!(provider.resolve_identity(&unsupported).is_err());
    assert!(provider.create(&unsupported).is_err());

    let mut first = provider
        .create(&descriptor.parameters)
        .expect("first body runtime");
    let first_stimulus = first
        .resolve_action(&BodyAction {
            coordinate: TemporalCoordinate::frame_start(0),
            action: "press_button".into(),
            value: json!({"player": 0, "button": "a"}),
        })
        .expect("press player-one A");
    assert_eq!(first_stimulus[0].port, "controllers_in");
    assert_eq!(first_stimulus[0].value, json!({"mask": 16}));

    let mut fresh = provider
        .create(&descriptor.parameters)
        .expect("fresh body runtime");
    let fresh_stimulus = fresh
        .resolve_action(&BodyAction {
            coordinate: TemporalCoordinate::frame_start(0),
            action: "press_button".into(),
            value: json!({"player": 1, "button": "b"}),
        })
        .expect("press player-two B");
    assert_eq!(fresh_stimulus[0].value, json!({"mask": 8192}));
    assert!(
        fresh
            .resolve_action(&BodyAction {
                coordinate: TemporalCoordinate::frame_start(1),
                action: "press_button".into(),
                value: json!({"player": 0, "button": "a", "extra": false}),
            })
            .is_err()
    );

    let request = ExecutionRequest::new(
        "pico8-default-body-session",
        "pico8",
        config(),
        ObservationPlan::fitness(),
    )
    .expect("request");
    let session = plugin
        .emulator()
        .create_execution(&artifact, request)
        .expect("materialized session");
    let context = session
        .request()
        .episode
        .clone()
        .expect("resolved episode before RunStarted");
    assert_eq!(context.body, identity);
    assert_eq!(context.interaction_policy.id, "glassvm.input.none.v1");

    let mut forged = context;
    forged.body.id = "pico8.forged".into();
    let forged_request = ExecutionRequest::new(
        "pico8-forged-body-session",
        "pico8",
        config(),
        ObservationPlan::fitness(),
    )
    .expect("request")
    .with_resolved_episode(forged)
    .expect("structurally valid forged context");
    let error = plugin
        .emulator()
        .create_execution(&artifact, forged_request)
        .err()
        .expect("forged body must fail");
    assert!(error.contains("does not match supported body"));
}

#[test]
fn scheduled_player_two_body_action_reaches_the_cartridge_input_api() {
    let artifact = b"pico-8 cartridge // http://www.pico-8.com\nversion 42\n__lua__\nfunction _update()\n if btn(5,1) and btnp(5,1) then\n  pset(0,0,9)\n else\n  pset(0,0,2)\n end\nend\n";
    let plugin = Pico8Plugin::new();
    let mut run_config = config();
    run_config.max_frames = 1;
    let request = EpisodeRequest::native_no_input(
        "pico8-player-two-body-action",
        "pico8",
        run_config,
        ObservationPlan::fitness(),
    )
    .expect("episode request")
    .with_interaction(InteractionPlan::Scheduled {
        actions: vec![BodyAction {
            coordinate: TemporalCoordinate::frame_start(0),
            action: "press_button".into(),
            value: json!({"player": 1, "button": "b"}),
        }],
        policy_seed: None,
    });
    let plan = resolve(&plugin, request).expect("resolve player-two body action");

    assert_eq!(plan.execution.stimuli.len(), 1);
    assert_eq!(
        plan.execution.stimuli[0].value,
        json!({"mask": 1_u16 << 13})
    );
    let mut collector = TraceCollector::new();
    let result = execute(&plugin, artifact, &plan, &mut collector)
        .expect("execute cartridge with player-two input");
    let framebuffer = result
        .capabilities
        .iter()
        .find(|capability| capability.key == "display.framebuffer_flat")
        .expect("framebuffer capability");
    assert_eq!(framebuffer.payload["bytes"][0], json!(9));
}

#[test]
fn native_event_adapter_normalizes_display_write() {
    let plugin = Pico8Plugin::new();
    let context = EventContext {
        arch: MachineId::from("pico8"),
        machine_version: VersionStamp::from(pico8_core::SEMANTICS),
        run_id: RunId::from("pico8-adapter-contract"),
        sequence: 5,
        step: 3,
        cycle_or_tick: Some(3),
        frame: Some(2),
        pc: None,
        instruction: None,
    };
    let native = NativeEvent {
        schema: SchemaRef::new("pico8.event", SchemaVersion::V1),
        kind: "display_write".into(),
        payload: json!({"x": 7, "y": 9, "color": 11}),
    };

    let events = plugin
        .native_event_adapter()
        .normalize(&context, &native)
        .expect("normalize PICO-8 display event");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, EventKind::DisplayWrite);
    assert_eq!(events[0].frame, context.frame);
    assert_eq!(events[0].extensions["pico8.native"], native.payload);
}

#[test]
fn observable_adapter_normalizes_flat_framebuffer() {
    let plugin = Pico8Plugin::new();
    let payload = json!({
        "width": 128,
        "height": 128,
        "planes": 1,
        "bytes": [0, 1, 2, 3]
    });
    let observations = [NativeObservation {
        schema: SchemaRef::new("pico8.framebuffer", SchemaVersion::V1),
        name: "pico8.framebuffer_flat".into(),
        payload: payload.clone(),
    }];

    let capabilities = plugin
        .observable_adapter()
        .normalize(&observations)
        .expect("normalize PICO-8 framebuffer observation");

    assert_eq!(capabilities.len(), 1);
    assert_eq!(capabilities[0].key, "display.framebuffer_flat");
    assert_eq!(capabilities[0].payload, payload);
}

#[test]
fn actual_p8_cart_loads_executes_observes_resets_and_snapshots() {
    let artifact = fs::read(SMOKE_ROM).expect("read checked-in PICO-8 cartridge");
    let cartridge = Cartridge::parse(&artifact).expect("parse native text cartridge");
    assert_eq!(cartridge.format, CartridgeFormat::P8Text);
    assert_eq!(cartridge.version, 41);
    assert!(cartridge.lua.contains("function _draw()"));

    let plugin = Pico8Plugin::new();
    plugin.validate_contract().expect("valid contract");
    let analysis = plugin
        .static_analyzer()
        .expect("analyzer")
        .analyze_bytes(&artifact)
        .expect("analyze cartridge");
    assert_eq!(
        analysis.capabilities[0].payload["artifact_format"],
        "p8-text"
    );
    let verification = plugin
        .verifier()
        .expect("verifier")
        .verify_bytes(&artifact)
        .expect("verify cartridge");
    assert_eq!(verification.severity_max, "warning");
    assert!(
        verification
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic["severity"] != "error")
    );

    let mut observation = ObservationPlan::ui();
    observation.snapshots = SnapshotCapture::EverySteps(1);
    let request =
        ExecutionRequest::new("pico8-actual-p8", "pico8", config(), observation).expect("request");
    let mut session = plugin
        .emulator()
        .create_execution(&artifact, request.clone())
        .expect("load cartridge");
    let mut collector = TraceCollector::new();
    let result = session.execute(&mut collector).expect("execute cartridge");
    let collected = collector.finish();
    assert_eq!(result.common.frames, 2);
    assert!(result.common.boot_success);
    assert!(
        result
            .capabilities
            .iter()
            .any(|capability| capability.key == "display.framebuffer_flat")
    );
    assert!(
        collected
            .events
            .iter()
            .any(|event| event.kind == EventKind::FrameCompleted)
    );
    assert!(
        collected
            .events
            .iter()
            .any(|event| event.extensions.contains_key("pico8.native")),
        "actual execution must pass through the native-event adapter"
    );
    assert!(collected.replay_manifest.is_some());
    assert!(collected.replay_receipt.is_some());
    assert!(collected.replay_package.is_some());
    assert_eq!(collected.snapshots.len(), 2);
    for snapshot in &collected.snapshots {
        snapshot
            .validate_integrity()
            .expect("continuation artifact integrity");
    }
    let emitted_continuation = collected.snapshots[0].bytes.clone();
    let mut emitted_restore = plugin
        .emulator()
        .create_execution(&artifact, request.clone())
        .expect("create emitted-snapshot target");
    emitted_restore
        .restore_snapshot(&emitted_continuation)
        .expect("restore emitted continuation");
    assert_eq!(
        emitted_restore.snapshot().expect("emitted round trip"),
        emitted_continuation
    );
    emitted_restore
        .step_frame()
        .expect("continue emitted mid-run snapshot");
    assert_eq!(
        emitted_restore
            .snapshot()
            .expect("emitted continuation final"),
        session.snapshot().expect("executed final continuation")
    );

    session.reset().expect("reset after execution");
    let boot_snapshot = session.snapshot().expect("boot snapshot");
    session.step_frame().expect("step cartridge");
    let advanced_snapshot = session.snapshot().expect("advanced snapshot");
    assert_ne!(advanced_snapshot, boot_snapshot);
    let mut restored = plugin
        .emulator()
        .create_execution(&artifact, request)
        .expect("create continuation target");
    restored
        .restore_snapshot(&boot_snapshot)
        .expect("restore boot continuation");
    assert_eq!(restored.snapshot().expect("boot round trip"), boot_snapshot);
    restored.reset().expect("reset before second restore");
    restored
        .restore_snapshot(&advanced_snapshot)
        .expect("restore advanced continuation");
    assert_eq!(
        restored.snapshot().expect("advanced round trip"),
        advanced_snapshot
    );
    session.step_frame().expect("continue original cartridge");
    restored.step_frame().expect("continue restored cartridge");
    assert_eq!(
        restored.snapshot().expect("restored final"),
        session.snapshot().expect("original final")
    );
    session.reset().expect("second reset");
    assert_eq!(session.snapshot().expect("reset snapshot"), boot_snapshot);
}

#[test]
fn disabling_summaries_changes_only_capability_output() {
    let artifact = fs::read(SMOKE_ROM).expect("fixture");
    let plugin = Pico8Plugin::new();
    let run = |run_id: &str, collect_summaries: bool| {
        let mut observation = ObservationPlan::debug();
        observation.collect_summaries = collect_summaries;
        let request =
            ExecutionRequest::new(run_id, "pico8", config(), observation).expect("request");
        let mut session = plugin
            .emulator()
            .create_execution(&artifact, request)
            .expect("session");
        let mut collector = TraceCollector::new();
        let result = session.execute(&mut collector).expect("execute");
        (result, collector.finish())
    };
    let (enabled, enabled_run) = run("pico8-summary-on", true);
    let (disabled, disabled_run) = run("pico8-summary-off", false);
    assert_eq!(enabled.common, disabled.common);
    assert!(!enabled.capabilities.is_empty());
    assert!(!enabled_run.capabilities.is_empty());
    assert!(disabled.capabilities.is_empty());
    assert!(disabled_run.capabilities.is_empty());
}

#[test]
fn native_payload_filters_preserve_universal_events_and_results() {
    let artifact = fs::read(SMOKE_ROM).expect("fixture");
    let plugin = Pico8Plugin::new();
    let run = |run_id: &str,
               native_events: glassvm_core::NativeEventSelection,
               include_extensions: bool| {
        let mut observation = ObservationPlan::debug();
        observation.native_events = native_events;
        observation.include_extensions = include_extensions;
        let request =
            ExecutionRequest::new(run_id, "pico8", config(), observation).expect("request");
        let mut session = plugin
            .emulator()
            .create_execution(&artifact, request)
            .expect("session");
        let mut collector = TraceCollector::new();
        let result = session.execute(&mut collector).expect("execute");
        (result, collector.finish())
    };
    let (without_native, none_run) = run(
        "pico8-native-none",
        glassvm_core::NativeEventSelection::None,
        true,
    );
    let (without_extensions, disabled_run) = run(
        "pico8-extensions-off",
        glassvm_core::NativeEventSelection::All,
        false,
    );
    assert_eq!(without_native, without_extensions);
    for collected in [none_run, disabled_run] {
        assert!(!collected.events.is_empty());
        assert!(
            collected
                .events
                .iter()
                .all(|event| event.extensions.is_empty())
        );
        assert!(
            collected
                .events
                .iter()
                .any(|event| event.kind == EventKind::FrameCompleted)
        );
    }
}
