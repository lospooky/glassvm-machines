use std::collections::BTreeMap;

use glassvm_core::{
    ExecutionRequest, MachineBundle, ObservationPlan, ReplayStimulus, RunConfig, SnapshotCapture,
    TemporalCoordinate, TraceCollector, canonical_json_fingerprint,
};
use serde_json::{Value, json};
use tic80_core::MACHINE_ID;
use tic80_plugin::Tic80Plugin;

const SMOKE: &[u8] = include_bytes!("../../fixtures/smoke.rom");

fn resign_snapshot(snapshot: &mut Value) {
    let mut payload = snapshot.clone();
    payload
        .as_object_mut()
        .expect("snapshot object")
        .remove("payload_digest");
    snapshot["payload_digest"] = serde_json::to_value(
        canonical_json_fingerprint("glassvm.tic80.replay-snapshot.v3", &payload)
            .expect("snapshot digest"),
    )
    .expect("digest JSON");
}

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

fn request(run_id: &str, max_frames: u64) -> ExecutionRequest {
    ExecutionRequest::new(
        run_id,
        MACHINE_ID,
        config(max_frames),
        ObservationPlan::debug(),
    )
    .expect("request")
}

#[test]
fn periodic_snapshots_are_truthful_restorable_frame_boundaries() {
    let plugin = Tic80Plugin::new();
    let mut observation = ObservationPlan::debug();
    observation.snapshots = SnapshotCapture::EverySteps(1);
    observation.trace_chunk_events = None;
    let request = ExecutionRequest::new(
        "tic80-periodic-snapshots",
        MACHINE_ID,
        config(2),
        observation.clone(),
    )
    .unwrap();
    let mut source = plugin
        .emulator()
        .create_execution(SMOKE, request.clone())
        .unwrap();
    let mut collector = TraceCollector::new();
    source.execute(&mut collector).unwrap();
    let snapshots = collector.finish().snapshots;
    assert_eq!(snapshots.len(), 2);
    for snapshot in &snapshots {
        snapshot.validate_integrity().unwrap();
        assert_eq!(snapshot.run_id.as_str(), "tic80-periodic-snapshots");
        assert_eq!(snapshot.arch.as_str(), MACHINE_ID);
    }
    let first: Value = serde_json::from_slice(&snapshots[0].bytes).unwrap();
    let second: Value = serde_json::from_slice(&snapshots[1].bytes).unwrap();
    assert_eq!(first["lifecycle"], "incremental");
    assert_eq!(second["lifecycle"], "terminal");

    let mut resumed = plugin
        .emulator()
        .create_execution(SMOKE, request.clone())
        .unwrap();
    resumed.restore_snapshot(&snapshots[0].bytes).unwrap();
    resumed.step_frame().unwrap();
    let resumed: Value = serde_json::from_slice(&resumed.snapshot().unwrap()).unwrap();
    assert_eq!(resumed["input_history"], second["input_history"]);

    let mut terminal = plugin.emulator().create_execution(SMOKE, request).unwrap();
    terminal.restore_snapshot(&snapshots[1].bytes).unwrap();
    assert!(terminal.step_frame().is_err());
    assert!(terminal.execute(&mut TraceCollector::new()).is_err());

    observation.snapshots = SnapshotCapture::None;
    let request =
        ExecutionRequest::new("tic80-no-snapshots", MACHINE_ID, config(1), observation).unwrap();
    let mut none = plugin.emulator().create_execution(SMOKE, request).unwrap();
    let mut collector = TraceCollector::new();
    none.execute(&mut collector).unwrap();
    assert!(collector.finish().snapshots.is_empty());
}

#[test]
fn snapshot_restore_resumes_exactly() {
    let plugin = Tic80Plugin::new();
    let mut fresh_source = plugin
        .emulator()
        .create_execution(SMOKE, request("tic80-fresh-snapshot", 2))
        .expect("fresh source");
    let fresh = fresh_source.snapshot().expect("fresh snapshot");
    let fresh_json: Value = serde_json::from_slice(&fresh).expect("fresh JSON");
    assert_eq!(fresh_json["version"], 3);
    assert_eq!(fresh_json["lifecycle"], "fresh");
    assert!(fresh_json.get("executed").is_none());
    let mut fresh_restored = plugin
        .emulator()
        .create_execution(SMOKE, request("tic80-fresh-snapshot", 2))
        .expect("fresh restore target");
    fresh_restored
        .restore_snapshot(&fresh)
        .expect("restore fresh snapshot");
    assert_eq!(fresh_restored.snapshot().unwrap(), fresh);
    let mut source_trace = TraceCollector::new();
    let mut restored_trace = TraceCollector::new();
    assert_eq!(
        fresh_source.execute(&mut source_trace).unwrap(),
        fresh_restored.execute(&mut restored_trace).unwrap()
    );
    assert_eq!(source_trace.finish(), restored_trace.finish());
    let terminal: Value =
        serde_json::from_slice(&fresh_source.snapshot().unwrap()).expect("terminal JSON");
    assert_eq!(terminal["lifecycle"], "terminal");

    let mut original = plugin
        .emulator()
        .create_execution(SMOKE, request("tic80-snapshot-resume", 2))
        .expect("original session");
    original.step_frame().expect("first frame");
    let checkpoint = original.snapshot().expect("checkpoint");
    assert_eq!(
        serde_json::from_slice::<Value>(&checkpoint).unwrap()["lifecycle"],
        "incremental"
    );
    original.step_frame().expect("second frame");
    let expected = original.snapshot().expect("expected continuation");
    assert!(original.restore_snapshot(&checkpoint).is_err());
    assert_eq!(original.snapshot().unwrap(), expected);

    let mut restored = plugin
        .emulator()
        .create_execution(SMOKE, request("tic80-snapshot-resume", 2))
        .expect("restored session");
    restored
        .restore_snapshot(&checkpoint)
        .expect("restore checkpoint");
    assert_eq!(restored.snapshot().expect("round trip"), checkpoint);
    restored.step_frame().expect("restored second frame");
    assert_eq!(
        restored.snapshot().expect("restored continuation"),
        expected
    );
}

#[test]
fn scheduled_inputs_are_exact_and_cannot_be_overwritten_or_extended_past_budget() {
    let plugin = Tic80Plugin::new();
    let scheduled_request = request("tic80-scheduled-input", 2)
        .with_stimuli(vec![
            ReplayStimulus {
                ordinal: 0,
                coordinate: TemporalCoordinate::frame_start(0),
                port: "gamepad_in".into(),
                value: json!({"mask": 1}),
            },
            ReplayStimulus {
                ordinal: 1,
                coordinate: TemporalCoordinate::frame_start(1),
                port: "gamepad_in".into(),
                value: json!({"mask": 1_u32 << 22}),
            },
        ])
        .expect("scheduled request");

    let mut blocked_input = plugin
        .emulator()
        .create_execution(SMOKE, scheduled_request.clone())
        .expect("blocked-input session");
    let before = blocked_input.snapshot().expect("before blocked input");
    assert!(blocked_input.set_input_mask(7).is_err());
    assert_eq!(
        blocked_input.snapshot().expect("unchanged input state"),
        before
    );

    let mut session = plugin
        .emulator()
        .create_execution(SMOKE, scheduled_request)
        .expect("scheduled session");
    session.step_frame().expect("first scheduled frame");
    let first: Value = serde_json::from_slice(&session.snapshot().expect("first snapshot"))
        .expect("decode first snapshot");
    assert_eq!(first["input_history"], json!([1]));
    assert_eq!(first["next_stimulus"], 1);
    let first_bytes = session.snapshot().expect("first bytes");
    let mut tampered_history = first.clone();
    tampered_history["input_history"] = json!([2]);
    tampered_history["input_mask"] = json!(2);
    resign_snapshot(&mut tampered_history);
    assert!(
        session
            .restore_snapshot(
                &serde_json::to_vec(&tampered_history).expect("encode tampered history")
            )
            .is_err()
    );
    assert_eq!(
        session.snapshot().expect("unchanged scheduled history"),
        first_bytes
    );
    let mut pending_override = first.clone();
    pending_override["input_override_pending"] = json!(true);
    resign_snapshot(&mut pending_override);
    assert!(
        session
            .restore_snapshot(
                &serde_json::to_vec(&pending_override).expect("encode pending override")
            )
            .is_err()
    );
    assert_eq!(
        session.snapshot().expect("unchanged pending override"),
        first_bytes
    );

    session.step_frame().expect("second scheduled frame");
    let complete = session.snapshot().expect("complete schedule");
    let decoded: Value = serde_json::from_slice(&complete).expect("decode complete snapshot");
    assert_eq!(decoded["input_history"], json!([1, 1_u32 << 22]));
    assert_eq!(decoded["next_stimulus"], 2);
    let mut pending_beyond_budget = decoded.clone();
    pending_beyond_budget["input_override_pending"] = json!(true);
    resign_snapshot(&mut pending_beyond_budget);
    assert!(
        session
            .restore_snapshot(
                &serde_json::to_vec(&pending_beyond_budget).expect("encode pending beyond budget")
            )
            .is_err()
    );
    assert_eq!(
        session.snapshot().expect("unchanged pending beyond budget"),
        complete
    );
    assert!(session.step_frame().is_err());
    assert_eq!(
        session.snapshot().expect("unchanged after excess step"),
        complete
    );
    assert!(session.set_input_mask(0).is_err());
    assert_eq!(
        session.snapshot().expect("unchanged after excess input"),
        complete
    );

    let same_frame_request = request("tic80-same-frame-input", 1)
        .with_stimuli(vec![
            ReplayStimulus {
                ordinal: 0,
                coordinate: TemporalCoordinate::frame_start(0),
                port: "gamepad_in".into(),
                value: json!({"mask": 1}),
            },
            ReplayStimulus {
                ordinal: 1,
                coordinate: TemporalCoordinate::frame_start(0),
                port: "gamepad_in".into(),
                value: json!({"mask": 2}),
            },
        ])
        .expect("same-frame schedule");
    let mut same_frame = plugin
        .emulator()
        .create_execution(SMOKE, same_frame_request)
        .expect("same-frame session");
    same_frame.step_frame().expect("same-frame final mask");
    let exact = same_frame.snapshot().expect("same-frame snapshot");
    let mut wrong_final: Value = serde_json::from_slice(&exact).expect("decode same-frame");
    assert_eq!(wrong_final["input_history"], json!([2]));
    wrong_final["input_history"] = json!([1]);
    wrong_final["input_mask"] = json!(1);
    resign_snapshot(&mut wrong_final);
    assert!(
        same_frame
            .restore_snapshot(&serde_json::to_vec(&wrong_final).expect("encode wrong final mask"))
            .is_err()
    );
    assert_eq!(
        same_frame.snapshot().expect("unchanged same-frame state"),
        exact
    );
}

#[test]
fn directly_deserialized_stimuli_are_revalidated_strictly() {
    let plugin = Tic80Plugin::new();
    let cases = [
        (
            "ordinal",
            json!([{"ordinal": 2, "coordinate": {"step": null, "cycle_or_tick": null, "frame": 0}, "port": "gamepad_in", "value": {"mask": 1}}]),
        ),
        (
            "not frame start",
            json!([{"ordinal": 0, "coordinate": {"step": 0, "cycle_or_tick": null, "frame": 0}, "port": "gamepad_in", "value": {"mask": 1}}]),
        ),
        (
            "out of order",
            json!([
                {"ordinal": 0, "coordinate": {"step": null, "cycle_or_tick": null, "frame": 1}, "port": "gamepad_in", "value": {"mask": 1}},
                {"ordinal": 1, "coordinate": {"step": null, "cycle_or_tick": null, "frame": 0}, "port": "gamepad_in", "value": {"mask": 1}}
            ]),
        ),
        (
            "outside budget",
            json!([{"ordinal": 0, "coordinate": {"step": null, "cycle_or_tick": null, "frame": 2}, "port": "gamepad_in", "value": {"mask": 1}}]),
        ),
        (
            "wrong port",
            json!([{"ordinal": 0, "coordinate": {"step": null, "cycle_or_tick": null, "frame": 0}, "port": "keyboard_in", "value": {"mask": 1}}]),
        ),
        (
            "non-object mask",
            json!([{"ordinal": 0, "coordinate": {"step": null, "cycle_or_tick": null, "frame": 0}, "port": "gamepad_in", "value": 1}]),
        ),
        (
            "extra mask field",
            json!([{"ordinal": 0, "coordinate": {"step": null, "cycle_or_tick": null, "frame": 0}, "port": "gamepad_in", "value": {"mask": 1, "extra": true}}]),
        ),
        (
            "wrong mask type",
            json!([{"ordinal": 0, "coordinate": {"step": null, "cycle_or_tick": null, "frame": 0}, "port": "gamepad_in", "value": {"mask": "1"}}]),
        ),
        (
            "oversized mask",
            json!([{"ordinal": 0, "coordinate": {"step": null, "cycle_or_tick": null, "frame": 0}, "port": "gamepad_in", "value": {"mask": u64::from(u32::MAX) + 1}}]),
        ),
    ];

    for (name, stimuli) in cases {
        let mut encoded = serde_json::to_value(request(name, 2)).expect("encode request");
        encoded["stimuli"] = stimuli;
        let deserialized: ExecutionRequest =
            serde_json::from_value(encoded).expect("direct request deserialization");
        assert!(
            plugin
                .emulator()
                .create_execution(SMOKE, deserialized)
                .is_err(),
            "directly deserialized {name} case must fail"
        );
    }
}

#[test]
fn rejected_snapshot_restores_are_atomic_and_budget_bounded() {
    let plugin = Tic80Plugin::new();
    let mut session = plugin
        .emulator()
        .create_execution(SMOKE, request("tic80-atomic-restore", 2))
        .expect("session");
    session.step_frame().expect("establish nonboot state");
    let before = session.snapshot().expect("state before rejection");
    let original: Value = serde_json::from_slice(&before).expect("snapshot JSON");
    assert_eq!(original["version"], 3);
    assert_eq!(original["lifecycle"], "incremental");

    assert!(session.restore_snapshot(b"{").is_err());
    assert_eq!(
        session.snapshot().expect("unchanged malformed restore"),
        before
    );

    let mut unknown: Value = serde_json::from_slice(&before).expect("decode snapshot");
    unknown["unknown"] = json!(true);
    resign_snapshot(&mut unknown);
    assert!(
        session
            .restore_snapshot(&serde_json::to_vec(&unknown).expect("encode unknown field"))
            .is_err()
    );
    assert_eq!(
        session.snapshot().expect("unchanged unknown restore"),
        before
    );

    let mut nested_unknown = original.clone();
    nested_unknown["payload_digest"]["unknown"] = json!(true);
    assert!(
        session
            .restore_snapshot(
                &serde_json::to_vec(&nested_unknown).expect("encode nested unknown field")
            )
            .is_err()
    );
    assert_eq!(
        session
            .snapshot()
            .expect("unchanged nested-unknown restore"),
        before
    );

    let mut legacy_version = original.clone();
    legacy_version["version"] = json!(2);
    resign_snapshot(&mut legacy_version);
    assert!(
        session
            .restore_snapshot(&serde_json::to_vec(&legacy_version).expect("legacy version"))
            .is_err()
    );
    assert_eq!(session.snapshot().unwrap(), before);

    let mut invalid_lifecycle = original.clone();
    invalid_lifecycle["lifecycle"] = json!("failed");
    resign_snapshot(&mut invalid_lifecycle);
    assert!(
        session
            .restore_snapshot(&serde_json::to_vec(&invalid_lifecycle).expect("bad lifecycle"))
            .is_err()
    );
    assert_eq!(session.snapshot().unwrap(), before);

    let mut false_fresh = original.clone();
    false_fresh["lifecycle"] = json!("fresh");
    resign_snapshot(&mut false_fresh);
    assert!(
        session
            .restore_snapshot(&serde_json::to_vec(&false_fresh).expect("false fresh"))
            .is_err()
    );
    assert_eq!(session.snapshot().unwrap(), before);

    let mut over_budget: Value = serde_json::from_slice(&before).expect("decode snapshot");
    over_budget["input_history"] = json!([0, 0, 0]);
    over_budget["input_mask"] = json!(0);
    resign_snapshot(&mut over_budget);
    assert!(
        session
            .restore_snapshot(&serde_json::to_vec(&over_budget).expect("encode over-budget"))
            .is_err()
    );
    assert_eq!(
        session.snapshot().expect("unchanged over-budget restore"),
        before
    );

    let failing_cart = lua_cart("function TIC() while true do end end");
    let mut failing = plugin
        .emulator()
        .create_execution(&failing_cart, request("tic80-atomic-runtime-restore", 2))
        .expect("failing session");
    let pristine = failing.snapshot().expect("pristine snapshot");
    let mut replay_failure: Value = serde_json::from_slice(&pristine).expect("decode snapshot");
    replay_failure["input_history"] = json!([0]);
    replay_failure["input_mask"] = json!(0);
    resign_snapshot(&mut replay_failure);
    assert!(
        failing
            .restore_snapshot(&serde_json::to_vec(&replay_failure).expect("encode replay failure"))
            .is_err()
    );
    assert_eq!(failing.snapshot().expect("atomic replay failure"), pristine);
}
