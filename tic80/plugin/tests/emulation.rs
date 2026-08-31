use std::collections::BTreeMap;

use glassvm_core::{
    CausalRelation, Emission, EmissionSink, EventContext, EventKind, ExecutionRequest,
    MachineBundle, NativeEvent, NativeEventSelection, NativeObservation, ObservationPlan,
    RunConfig, RunId, SchemaRef, SchemaVersion, SinkError, SnapshotCapture, TraceChunk,
    TraceCollector,
};
use serde_json::json;
use tic80_core::{MACHINE_ID, SEMANTICS};
use tic80_plugin::Tic80Plugin;

const SMOKE: &[u8] = include_bytes!("../../fixtures/smoke.rom");

fn lua_cart(source: &str) -> Vec<u8> {
    let mut cart = vec![5];
    cart.extend_from_slice(&(source.len() as u16).to_le_bytes());
    cart.push(0);
    cart.extend_from_slice(source.as_bytes());
    cart
}

fn config(max_frames: u64) -> RunConfig {
    RunConfig {
        max_frames,
        cycles_per_frame: 1,
        seed: 7,
        machine_params: BTreeMap::new(),
        record_frames: true,
        record_events: true,
    }
}

fn request(run_id: &str, config: RunConfig) -> ExecutionRequest {
    ExecutionRequest::new(run_id, MACHINE_ID, config, ObservationPlan::ui()).expect("request")
}

#[derive(Default)]
struct CountingSink {
    emissions: usize,
}

impl EmissionSink for CountingSink {
    fn emit(&mut self, _emission: Emission<'_>) -> Result<(), SinkError> {
        self.emissions += 1;
        Ok(())
    }
}

struct RejectingSink;

impl EmissionSink for RejectingSink {
    fn emit(&mut self, _emission: Emission<'_>) -> Result<(), SinkError> {
        Err(SinkError::new("intentional TIC-80 sink rejection"))
    }
}

#[test]
fn real_cartridge_native_events_flow_through_the_adapter_with_payload_and_causality() {
    let plugin = Tic80Plugin::new();
    let mut observation = ObservationPlan::ui();
    observation.snapshots = SnapshotCapture::Final;
    observation.trace_chunk_events = Some(2);
    let request = ExecutionRequest::new("tic80-native-events", MACHINE_ID, config(1), observation)
        .expect("request");
    let mut session = plugin
        .emulator()
        .create_execution(SMOKE, request)
        .expect("TIC-80 session");
    let expected_request = session.request().clone();
    let mut collector = TraceCollector::new();
    let result = session.execute(&mut collector).expect("execute cartridge");
    let trace = collector.finish();

    assert_eq!(result.common.frames, 1);
    assert!(result.common.boot_success);
    assert_eq!(trace.request.as_ref(), Some(&expected_request));
    assert_eq!(trace.result.as_ref(), Some(&result));
    assert_eq!(trace.capabilities, result.capabilities);
    assert_eq!(trace.snapshots.len(), 1);
    trace.snapshots[0]
        .validate_integrity()
        .expect("final snapshot integrity");
    TraceChunk::validate_chain(&trace.trace_chunks).expect("valid emitted trace chain");
    let chunk_events = trace
        .trace_chunks
        .iter()
        .flat_map(|chunk| chunk.events.iter().cloned())
        .collect::<Vec<_>>();
    assert_eq!(chunk_events, trace.events);
    assert!(trace.trace_chunks.iter().all(|chunk| chunk.header.filtered));

    let framebuffer = result
        .capabilities
        .iter()
        .find(|capability| capability.key == "tic80.framebuffer_rgba")
        .expect("adapter-normalized framebuffer");
    let frame_summary = result
        .capabilities
        .iter()
        .find(|capability| capability.key == "tic80.frame_summary")
        .expect("adapter-normalized frame summary");
    let expected_capabilities = plugin
        .observable_adapter()
        .normalize(&[
            NativeObservation {
                schema: SchemaRef::new("tic80.framebuffer", SchemaVersion::V1),
                name: "framebuffer_rgba".into(),
                payload: framebuffer.payload.clone(),
            },
            NativeObservation {
                schema: SchemaRef::new("tic80.execution", SchemaVersion::V1),
                name: "frame_summary".into(),
                payload: frame_summary.payload.clone(),
            },
        ])
        .expect("normalize actual TIC-80 observations through declared adapter");
    assert_eq!(result.capabilities, expected_capabilities);

    let input = trace
        .events
        .iter()
        .find(|event| event.kind == EventKind::InputSampled)
        .expect("normalized native input event");
    assert_eq!(input.arch.as_str(), MACHINE_ID);
    assert_eq!(input.machine_version.as_str(), SEMANTICS);
    assert_eq!(input.run_id, RunId::from("tic80-native-events"));
    assert_eq!((input.step, input.frame), (1, Some(1)));
    assert_eq!(input.extensions["tic80.input"], json!({"mask": 0}));
    assert_eq!(input.io[0].port, "gamepad_in");
    let normalized_input = plugin
        .native_event_adapter()
        .normalize(
            &EventContext {
                arch: input.arch.clone(),
                machine_version: input.machine_version.clone(),
                run_id: input.run_id.clone(),
                sequence: input.sequence,
                step: input.step,
                cycle_or_tick: input.cycle_or_tick,
                frame: input.frame,
                pc: input.pc.clone(),
                instruction: input.instruction.clone(),
            },
            &NativeEvent {
                schema: SchemaRef::new("tic80.event", SchemaVersion::V1),
                kind: "input".into(),
                payload: json!({"mask": 0}),
            },
        )
        .expect("normalize actual native input through declared adapter");
    let mut adapter_owned_input = input.clone();
    adapter_owned_input.io.clear();
    assert_eq!(normalized_input, [adapter_owned_input]);

    let frame = trace
        .events
        .iter()
        .find(|event| event.kind == EventKind::FrameCompleted)
        .expect("normalized native frame event");
    assert_eq!(frame.extensions["tic80.frame"], json!({"frame": 1}));
    assert_eq!(frame.run_id, input.run_id);
    assert_eq!((frame.step, frame.frame), (1, Some(1)));
    assert!(input.sequence < frame.sequence);
    assert_eq!(frame.io[0].port, "display_out");
    assert!(frame.io[0].value["sha256"].is_string());
    assert!(frame.causes.iter().any(|cause| {
        cause.source_sequence == input.sequence && cause.relation == CausalRelation::Input
    }));
    let normalized_frame = plugin
        .native_event_adapter()
        .normalize(
            &EventContext {
                arch: frame.arch.clone(),
                machine_version: frame.machine_version.clone(),
                run_id: frame.run_id.clone(),
                sequence: frame.sequence,
                step: frame.step,
                cycle_or_tick: frame.cycle_or_tick,
                frame: frame.frame,
                pc: frame.pc.clone(),
                instruction: frame.instruction.clone(),
            },
            &NativeEvent {
                schema: SchemaRef::new("tic80.event", SchemaVersion::V1),
                kind: "frame".into(),
                payload: json!({"frame": 1}),
            },
        )
        .expect("normalize actual native frame through declared adapter");
    let mut adapter_owned_frame = frame.clone();
    adapter_owned_frame.io.clear();
    adapter_owned_frame.causes.clear();
    assert_eq!(normalized_frame, [adapter_owned_frame]);
}

#[test]
fn real_cartridge_payload_filters_summaries_and_fitness_lifecycle_are_exact() {
    let plugin = Tic80Plugin::new();
    let run = |observation| {
        let request =
            ExecutionRequest::new("tic80-filtered-native", MACHINE_ID, config(1), observation)
                .unwrap();
        let mut session = plugin.emulator().create_execution(SMOKE, request).unwrap();
        let mut collector = TraceCollector::new();
        let result = session.execute(&mut collector).unwrap();
        (result, collector.finish())
    };

    let mut native_none = ObservationPlan::ui();
    native_none.native_events = NativeEventSelection::None;
    native_none.include_extensions = true;
    native_none.collect_summaries = false;
    native_none.snapshots = SnapshotCapture::None;
    native_none.trace_chunk_events = None;
    let mut extensions_disabled = native_none.clone();
    extensions_disabled.native_events = NativeEventSelection::All;
    extensions_disabled.include_extensions = false;
    let (none_result, none) = run(native_none);
    let (disabled_result, disabled) = run(extensions_disabled);
    assert!(none_result.capabilities.is_empty() && disabled_result.capabilities.is_empty());
    assert!(none.capabilities.is_empty() && disabled.capabilities.is_empty());
    assert!(!none.events.is_empty());
    assert_eq!(none.events, disabled.events);
    assert!(none.events.iter().all(|event| event.extensions.is_empty()));
    assert!(none.events.iter().any(|event| !event.io.is_empty()));
    assert!(none.events.iter().any(|event| !event.causes.is_empty()));
    for (index, event) in none.events.iter().enumerate() {
        assert_eq!(event.sequence, index as u64);
    }

    let mut fitness_config = config(1);
    fitness_config.record_frames = false;
    fitness_config.record_events = false;
    let request = ExecutionRequest::new(
        "tic80-fitness-lifecycle",
        MACHINE_ID,
        fitness_config,
        ObservationPlan::fitness(),
    )
    .unwrap();
    let mut session = plugin.emulator().create_execution(SMOKE, request).unwrap();
    let mut collector = TraceCollector::new();
    session.execute(&mut collector).unwrap();
    let events = collector.finish().events;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].kind, EventKind::RunStarted);
    assert_eq!(events[1].kind, EventKind::RunHalted);
    assert_eq!((events[0].sequence, events[1].sequence), (0, 1));
    assert!(events.iter().all(|event| event.extensions.is_empty()));
}

#[test]
fn native_runtime_configuration_is_strict_and_defaults_only_when_absent() {
    let plugin = Tic80Plugin::new();
    plugin
        .emulator()
        .create_execution(SMOKE, request("tic80-default-runtime", config(1)))
        .expect("absent runtime selects documented Lua default");

    let mut explicit = config(1);
    explicit
        .machine_params
        .insert("runtime".into(), json!("lua"));
    plugin
        .emulator()
        .create_execution(SMOKE, request("tic80-explicit-runtime", explicit))
        .expect("explicit Lua runtime");

    let invalid = [
        ("wrong runtime value", "runtime", json!("javascript"), 1),
        ("wrong runtime type", "runtime", json!(54), 1),
        ("unknown key", "region", json!("pal"), 1),
        ("wrong cycle ratio", "runtime", json!("lua"), 2),
    ];
    for (run_id, key, value, cycles_per_frame) in invalid {
        let mut invalid_config = config(1);
        invalid_config.cycles_per_frame = cycles_per_frame;
        invalid_config.machine_params.insert(key.into(), value);
        assert!(
            plugin
                .emulator()
                .create_execution(SMOKE, request(run_id, invalid_config))
                .is_err(),
            "{run_id} must fail"
        );
    }
}

#[test]
fn dirty_and_terminal_lifecycles_reject_execute_before_any_emission_and_reset_rearms() {
    let plugin = Tic80Plugin::new();

    let mut checkpoint_source = plugin
        .emulator()
        .create_execution(SMOKE, request("tic80-checkpoint", config(2)))
        .expect("checkpoint source");
    checkpoint_source.step_frame().expect("checkpoint frame");
    let checkpoint = checkpoint_source.snapshot().expect("checkpoint");

    let mut stepped = plugin
        .emulator()
        .create_execution(SMOKE, request("tic80-dirty-step", config(2)))
        .expect("stepped session");
    stepped.step_frame().expect("dirty step");
    let mut sink = CountingSink::default();
    assert!(stepped.execute(&mut sink).is_err());
    assert_eq!(sink.emissions, 0);

    let mut input_dirty = plugin
        .emulator()
        .create_execution(SMOKE, request("tic80-dirty-input", config(2)))
        .expect("input session");
    input_dirty.set_input_mask(1).expect("immediate input");
    let mut sink = CountingSink::default();
    assert!(input_dirty.execute(&mut sink).is_err());
    assert_eq!(sink.emissions, 0);

    let mut restored = plugin
        .emulator()
        .create_execution(SMOKE, request("tic80-checkpoint", config(2)))
        .expect("restore target");
    restored.restore_snapshot(&checkpoint).expect("restore");
    let mut sink = CountingSink::default();
    assert!(restored.execute(&mut sink).is_err());
    assert_eq!(sink.emissions, 0);

    let mut terminal = plugin
        .emulator()
        .create_execution(SMOKE, request("tic80-terminal", config(1)))
        .expect("terminal session");
    terminal
        .execute(&mut TraceCollector::new())
        .expect("terminal execution");
    let forensic = terminal.snapshot().expect("terminal forensic snapshot");
    assert!(terminal.step_frame().is_err());
    assert!(terminal.set_input_mask(1).is_err());
    assert!(terminal.restore_snapshot(&checkpoint).is_err());
    let mut sink = CountingSink::default();
    assert!(terminal.execute(&mut sink).is_err());
    assert_eq!(sink.emissions, 0);
    assert_eq!(
        terminal.snapshot().expect("unchanged forensic snapshot"),
        forensic
    );

    terminal.reset().expect("reset re-arms");
    terminal
        .execute(&mut TraceCollector::new())
        .expect("execute after reset");
}

#[test]
fn sink_and_runtime_failures_poison_the_session_until_reset() {
    let plugin = Tic80Plugin::new();
    let mut rejected = plugin
        .emulator()
        .create_execution(SMOKE, request("tic80-sink-failure", config(1)))
        .expect("sink session");
    assert!(rejected.execute(&mut RejectingSink).is_err());
    let forensic = rejected.snapshot().expect("forensic snapshot");
    assert!(rejected.step_frame().is_err());
    assert!(rejected.set_input_mask(1).is_err());
    assert!(rejected.restore_snapshot(&forensic).is_err());
    assert!(rejected.execute(&mut CountingSink::default()).is_err());
    assert_eq!(rejected.snapshot().expect("unchanged snapshot"), forensic);
    rejected.reset().expect("reset after sink failure");
    rejected
        .execute(&mut TraceCollector::new())
        .expect("execution after reset");

    let failing_cart = lua_cart("function TIC() while true do end end");
    let mut failed_step = plugin
        .emulator()
        .create_execution(&failing_cart, request("tic80-step-failure", config(1)))
        .expect("failing runtime");
    let boot = failed_step.snapshot().expect("boot snapshot");
    assert!(failed_step.step_frame().is_err());
    let forensic = failed_step.snapshot().expect("failed-step snapshot");
    assert!(failed_step.step_frame().is_err());
    assert!(failed_step.set_input_mask(1).is_err());
    assert!(failed_step.restore_snapshot(&boot).is_err());
    assert!(failed_step.execute(&mut CountingSink::default()).is_err());
    assert_eq!(
        failed_step.snapshot().expect("unchanged forensic state"),
        forensic
    );
    failed_step.reset().expect("reset failed step");
    assert!(failed_step.step_frame().is_err(), "runtime fails anew");

    let mut failed_execute = plugin
        .emulator()
        .create_execution(&failing_cart, request("tic80-execute-failure", config(1)))
        .expect("failing execute session");
    assert!(failed_execute.execute(&mut TraceCollector::new()).is_err());
    let forensic = failed_execute.snapshot().expect("execute-failure snapshot");
    assert!(failed_execute.step_frame().is_err());
    assert!(failed_execute.set_input_mask(0).is_err());
    assert!(failed_execute.restore_snapshot(&forensic).is_err());
    assert!(
        failed_execute
            .execute(&mut CountingSink::default())
            .is_err()
    );
    assert_eq!(
        failed_execute
            .snapshot()
            .expect("unchanged execute failure"),
        forensic
    );
}
