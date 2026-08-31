use std::collections::BTreeMap;

use glassvm_core::{
    ContentDigest, Emission, EmissionSink, EmulatorBackend, EventKind, ExecutionRequest,
    MachineBundle, ObservationPlan, ReplayStimulus, RunConfig, SinkError, TemporalCoordinate,
    TraceCollector, VerifierBackend, canonical_json_bytes, caps,
};
use pico8_plugin::{Pico8EmulatorBackend, Pico8Plugin, Pico8VerifierBackend};
use serde_json::{Value, json};

const SMOKE: &[u8] = include_bytes!("../../fixtures/smoke.rom");

fn request(run_id: &str) -> ExecutionRequest {
    ExecutionRequest::new(
        run_id,
        "pico8",
        RunConfig {
            max_frames: 2,
            cycles_per_frame: 100,
            seed: 17,
            machine_params: BTreeMap::new(),
            record_frames: true,
            record_events: true,
        },
        ObservationPlan::debug(),
    )
    .expect("execution request")
}

#[test]
fn snapshot_restore_resumes_to_the_exact_same_continuation() {
    let plugin = Pico8Plugin::new();
    let mut original = plugin
        .emulator()
        .create_execution(SMOKE, request("pico8-original"))
        .expect("original session");
    let boot = original.snapshot().expect("boot snapshot");
    original.step_frame().expect("advance original");
    let expected = original.snapshot().expect("advanced snapshot");

    let mut restored = plugin
        .emulator()
        .create_execution(SMOKE, request("pico8-original"))
        .expect("restored session");
    restored
        .restore_snapshot(&boot)
        .expect("restore boot state");
    restored.step_frame().expect("advance restored");

    assert_eq!(restored.snapshot().expect("restored snapshot"), expected);
}

fn cart(source: &str) -> Vec<u8> {
    format!("pico-8 cartridge // http://www.pico-8.com\nversion 42\n__lua__\n{source}\n")
        .into_bytes()
}

fn source_request(max_frames: u64, observation: ObservationPlan) -> ExecutionRequest {
    ExecutionRequest::new(
        "pico8-test",
        pico8_core::MACHINE_ID,
        RunConfig {
            max_frames,
            cycles_per_frame: 100,
            seed: 17,
            machine_params: BTreeMap::new(),
            record_frames: false,
            record_events: false,
        },
        observation,
    )
    .expect("request")
}

#[test]
fn bundle_contract_is_self_consistent() {
    Pico8Plugin::new()
        .validate_contract()
        .expect("valid contract");
}

#[test]
fn verifier_reports_invalid_artifacts_without_panicking() {
    let result = Pico8VerifierBackend
        .verify_bytes(b"not a cart")
        .expect("verify");
    assert_eq!(result.severity_max, "error");
    assert_eq!(result.diagnostics.len(), 1);
}

#[test]
fn execution_emits_frames_capabilities_and_replay_evidence() {
    let backend = Pico8EmulatorBackend;
    let mut session = backend
        .create_execution(
            &cart("x=0\nfunction _update60() x+=1 end\nfunction _draw() cls(0) pset(x,2,8) end"),
            source_request(2, ObservationPlan::ui()),
        )
        .expect("session");
    let mut collector = TraceCollector::new();
    let result = session.execute(&mut collector).expect("execute");
    let collected = collector.finish();
    assert_eq!(result.common.frames, 2);
    assert_eq!(result.common.termination, "frame_budget");
    assert!(
        collected
            .events
            .iter()
            .any(|event| event.kind == EventKind::FrameCompleted)
    );
    assert!(collected.replay_manifest.is_some());
    assert!(collected.replay_receipt.is_some());
    assert!(collected.replay_package.is_some());
    assert!(collected.result.is_some());
}

#[test]
fn controller_stimulus_is_applied_at_frame_start() {
    let request = source_request(1, ObservationPlan::ui())
        .with_stimuli(vec![ReplayStimulus {
            ordinal: 0,
            coordinate: TemporalCoordinate::frame_start(0),
            port: "controllers_in".into(),
            value: json!({"mask": 16}),
        }])
        .expect("stimuli");
    let mut session = Pico8EmulatorBackend
        .create_execution(
            &cart("function _update() if btn(4) then pset(0,0,9) end end"),
            request,
        )
        .expect("session");
    let result = session
        .execute(&mut TraceCollector::new())
        .expect("execute");
    let frame = result
        .capabilities
        .iter()
        .find(|capability| capability.key == caps::DISPLAY_FRAMEBUFFER_FLAT)
        .expect("frame");
    assert_eq!(frame.payload["bytes"][0], 9);
}

#[test]
fn replay_package_reproduces_receipt() {
    let artifact =
        cart("function _update() pset(flr(rnd(8)),0,7) end\nfunction _draw() pset(2,2,3) end");
    let mut session = Pico8EmulatorBackend
        .create_execution(&artifact, source_request(3, ObservationPlan::debug()))
        .expect("session");
    let mut collector = TraceCollector::new();
    session.execute(&mut collector).expect("execute");
    let package = collector.finish().replay_package.expect("replay package");
    let outcome = Pico8EmulatorBackend
        .replay_package(
            &artifact,
            &package,
            "pico8-replay",
            &mut TraceCollector::new(),
        )
        .expect("replay");
    assert!(outcome.is_match(), "{outcome:?}");
}

#[test]
fn wrong_artifact_fails_replay_preflight_without_emission() {
    let artifact = cart("function _draw() pset(1,1,2) end");
    let mut session = Pico8EmulatorBackend
        .create_execution(&artifact, source_request(1, ObservationPlan::debug()))
        .expect("session");
    let mut collector = TraceCollector::new();
    session.execute(&mut collector).expect("execute");
    let package = collector.finish().replay_package.expect("replay package");
    let mut wrong = artifact.clone();
    wrong.push(b' ');
    let mut replay_collector = TraceCollector::new();
    let outcome = Pico8EmulatorBackend
        .replay_package(&wrong, &package, "wrong-artifact", &mut replay_collector)
        .expect("preflight");
    assert!(outcome.result.is_none());
    assert!(!outcome.preflight.is_match());
    let emitted = replay_collector.finish();
    assert!(emitted.request.is_none());
    assert!(emitted.events.is_empty());
    assert!(emitted.result.is_none());
}

struct RejectingSink {
    seen: usize,
    reject_at: usize,
}

impl EmissionSink for RejectingSink {
    fn emit(&mut self, _emission: Emission<'_>) -> Result<(), SinkError> {
        self.seen += 1;
        if self.seen == self.reject_at {
            Err(SinkError::new("intentional test rejection"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn sink_rejection_is_terminal_until_reset() {
    let artifact = cart("function _draw() pset(1,1,2) end");
    let execution = source_request(2, ObservationPlan::ui())
        .with_stimuli(vec![ReplayStimulus {
            ordinal: 0,
            coordinate: TemporalCoordinate::frame_start(0),
            port: "controllers_in".into(),
            value: json!({"mask": 1}),
        }])
        .expect("stimuli");
    let mut session = Pico8EmulatorBackend
        .create_execution(&artifact, execution)
        .expect("session");
    let error = session
        .execute(&mut RejectingSink {
            seen: 0,
            reject_at: 3,
        })
        .expect_err("sink rejection");
    assert!(error.contains("sink rejected"));
    let terminal = session.snapshot().expect("terminal continuation");
    assert!(session.restore_snapshot(&terminal).is_err());
    session.reset().expect("reset before terminal restore");
    session
        .restore_snapshot(&terminal)
        .expect("restore terminal continuation into fresh session");
    assert!(
        session
            .execute(&mut TraceCollector::new())
            .expect_err("dirty session")
            .contains("must begin at boot")
    );
    session.reset().expect("reset restored terminal session");
    let result = session
        .execute(&mut TraceCollector::new())
        .expect("execute after reset");
    assert_eq!(result.common.frames, 2);
}

#[test]
fn observation_richness_does_not_change_machine_result() {
    let artifact = cart(
        "x=0\nfunction _update()\nx+=1\npset(x,0,flr(rnd(16)))\nend\nfunction _draw() pset(2,2,3) end",
    );
    let mut fitness = Pico8EmulatorBackend
        .create_execution(&artifact, source_request(3, ObservationPlan::fitness()))
        .expect("fitness");
    let mut forensics = Pico8EmulatorBackend
        .create_execution(&artifact, source_request(3, ObservationPlan::forensics()))
        .expect("forensics");
    let left = fitness
        .execute(&mut TraceCollector::new())
        .expect("fitness execute");
    let mut collector = TraceCollector::new();
    let right = forensics.execute(&mut collector).expect("rich execute");
    assert_eq!(left, right);
    let chunks = collector.finish().trace_chunks;
    assert!(!chunks.is_empty());
    assert!(chunks.iter().all(|chunk| chunk.header.filtered));
}

#[test]
fn runtime_error_is_a_replayable_terminal_result() {
    let artifact = cart("function _update() while true do end end");
    let execution = source_request(4, ObservationPlan::debug());
    let mut session = Pico8EmulatorBackend
        .create_execution(&artifact, execution.clone())
        .expect("session");
    let mut collector = TraceCollector::new();
    let result = session.execute(&mut collector).expect("terminal result");
    assert_eq!(result.common.termination, "runtime_error");
    assert_eq!(result.common.frames, 0);
    assert!(
        result
            .capabilities
            .iter()
            .any(|capability| capability.key == "pico8.runtime_error")
    );
    let collected = collector.finish();
    assert!(collected.replay_package.is_some());
    let continuation = &collected.snapshots[0].bytes;
    let mut restored = Pico8EmulatorBackend
        .create_execution(&artifact, execution)
        .expect("restore target");
    restored
        .restore_snapshot(continuation)
        .expect("restore runtime-error continuation");
    assert_eq!(
        restored.snapshot().expect("restored continuation"),
        *continuation
    );
    assert!(
        restored
            .step_frame()
            .expect_err("runtime error is terminal")
            .contains("terminal after runtime failure")
    );
}

#[test]
fn continuation_replays_hidden_lua_state_and_resets_exactly() {
    let artifact =
        cart("local hidden=0\nfunction _update()\nhidden+=1\npset(hidden,0,hidden)\nend");
    let execution = source_request(4, ObservationPlan::debug());
    let mut source = Pico8EmulatorBackend
        .create_execution(&artifact, execution.clone())
        .expect("source");
    let boot = source.snapshot().expect("boot continuation");
    source.step_frame().expect("frame 0");
    source.step_frame().expect("frame 1");
    let checkpoint = source.snapshot().expect("checkpoint");
    source.step_frame().expect("frame 2");
    source.step_frame().expect("frame 3");
    let expected = source.snapshot().expect("expected continuation");

    let mut restored = Pico8EmulatorBackend
        .create_execution(&artifact, execution)
        .expect("restore target");
    restored
        .restore_snapshot(&checkpoint)
        .expect("restore checkpoint");
    assert_eq!(restored.snapshot().expect("round trip"), checkpoint);
    restored.step_frame().expect("restored frame 2");
    restored.step_frame().expect("restored frame 3");
    assert_eq!(restored.snapshot().expect("restored final"), expected);

    source.reset().expect("reset");
    assert_eq!(source.snapshot().expect("reset continuation"), boot);
}

#[test]
fn step_frame_applies_scheduled_and_recorded_inputs_like_execute() {
    let artifact = cart("function _update() if btn(4) then pset(0,0,9) else pset(0,0,2) end end");
    let execution = source_request(2, ObservationPlan::debug())
        .with_stimuli(vec![
            ReplayStimulus {
                ordinal: 0,
                coordinate: TemporalCoordinate::frame_start(0),
                port: "controllers_in".into(),
                value: json!({"mask": 16}),
            },
            ReplayStimulus {
                ordinal: 1,
                coordinate: TemporalCoordinate::frame_start(1),
                port: "controllers_in".into(),
                value: json!({"mask": 0}),
            },
        ])
        .expect("stimuli");
    let mut executed = Pico8EmulatorBackend
        .create_execution(&artifact, execution.clone())
        .expect("execute session");
    executed
        .execute(&mut TraceCollector::new())
        .expect("execute");

    let mut stepped = Pico8EmulatorBackend
        .create_execution(&artifact, execution)
        .expect("step session");
    stepped.step_frame().expect("step frame 0");
    stepped.step_frame().expect("step frame 1");
    assert_eq!(
        stepped.snapshot().expect("stepped continuation"),
        executed.snapshot().expect("executed continuation")
    );
    assert!(
        stepped
            .step_frame()
            .expect_err("budget exhausted")
            .contains("frame budget")
    );

    let direct_request = source_request(2, ObservationPlan::debug());
    let mut direct = Pico8EmulatorBackend
        .create_execution(&artifact, direct_request.clone())
        .expect("direct input session");
    direct.set_input_mask(16).expect("record immediate input");
    direct.step_frame().expect("direct frame");
    let checkpoint = direct.snapshot().expect("direct checkpoint");
    let mut direct_restored = Pico8EmulatorBackend
        .create_execution(&artifact, direct_request)
        .expect("direct restore target");
    direct_restored
        .restore_snapshot(&checkpoint)
        .expect("restore recorded input");
    direct.step_frame().expect("direct continuation");
    direct_restored
        .step_frame()
        .expect("restored direct continuation");
    assert_eq!(
        direct_restored.snapshot().expect("restored direct final"),
        direct.snapshot().expect("direct final")
    );
}

#[test]
fn stimuli_must_be_ordered_and_frame_addressed() {
    let request = source_request(2, ObservationPlan::debug())
        .with_stimuli(vec![
            ReplayStimulus {
                ordinal: 0,
                coordinate: TemporalCoordinate::frame_start(1),
                port: "controllers_in".into(),
                value: json!({"mask": 0}),
            },
            ReplayStimulus {
                ordinal: 1,
                coordinate: TemporalCoordinate::frame_start(0),
                port: "controllers_in".into(),
                value: json!({"mask": 1}),
            },
        ])
        .expect("generic stimulus envelope");
    let error = Pico8EmulatorBackend
        .create_execution(&cart(""), request)
        .err()
        .expect("PICO-8 ordering rejection");
    assert!(error.contains("nondecreasing frame"));

    let request = source_request(1, ObservationPlan::debug())
        .with_stimuli(vec![ReplayStimulus {
            ordinal: 0,
            coordinate: TemporalCoordinate::frame_start(0),
            port: "controllers_in".into(),
            value: json!({"mask": 1, "extra": true}),
        }])
        .expect("generic stimulus envelope");
    let error = Pico8EmulatorBackend
        .create_execution(&cart(""), request)
        .err()
        .expect("extra stimulus fields must fail");
    assert!(error.contains("exactly {mask}"), "{error}");
}

#[test]
fn directly_deserialized_zero_cycles_per_frame_is_rejected() {
    let mut encoded = serde_json::to_value(RunConfig {
        max_frames: 1,
        cycles_per_frame: 100,
        seed: 17,
        machine_params: BTreeMap::new(),
        record_frames: false,
        record_events: false,
    })
    .expect("serialize config");
    encoded["cycles_per_frame"] = json!(0);
    let config: RunConfig = serde_json::from_value(encoded).expect("direct deserialize");
    let request = ExecutionRequest::new("zero-cycles", "pico8", config, ObservationPlan::debug())
        .expect("generic request accepts machine-specific validation");
    let error = Pico8EmulatorBackend
        .create_execution(&cart(""), request)
        .err()
        .expect("PICO-8 backend must reject zero cycles_per_frame");
    assert!(
        error.contains("cycles_per_frame must be non-zero"),
        "{error}"
    );
}

fn resign_continuation(continuation: &mut Value) {
    let integrity = json!({
        "domain": "pico8.continuation.integrity.v2",
        "schema_version": continuation["schema_version"].clone(),
        "machine_version": continuation["machine_version"].clone(),
        "emulator_version": continuation["emulator_version"].clone(),
        "artifact_digest": continuation["artifact_digest"].clone(),
        "request_digest": continuation["request_digest"].clone(),
        "runtime": continuation["runtime"].clone(),
        "stimuli": continuation["stimuli"].clone(),
        "applied_stimuli": continuation["applied_stimuli"].clone(),
        "lifecycle": continuation["lifecycle"].clone(),
        "runtime_error": continuation["runtime_error"].clone(),
    });
    let digest = ContentDigest::sha256(
        &canonical_json_bytes(&integrity).expect("canonical continuation integrity payload"),
    );
    continuation["integrity_digest"] =
        serde_json::to_value(digest).expect("serialize continuation integrity digest");
}

#[test]
fn continuation_rejects_foreign_tampered_and_cursor_inconsistent_state_transactionally() {
    let artifact = cart("local hidden=0\nfunction _update()\nhidden+=1\npoke(0,hidden)\nend");
    let execution = source_request(3, ObservationPlan::debug())
        .with_stimuli(vec![ReplayStimulus {
            ordinal: 0,
            coordinate: TemporalCoordinate::frame_start(0),
            port: "controllers_in".into(),
            value: json!({"mask": 16}),
        }])
        .expect("stimuli");
    let mut source = Pico8EmulatorBackend
        .create_execution(&artifact, execution.clone())
        .expect("source");
    source.step_frame().expect("advance source");
    let checkpoint = source.snapshot().expect("checkpoint");

    let foreign_artifact = cart("function _update() poke(0,99) end");
    let mut foreign = Pico8EmulatorBackend
        .create_execution(&foreign_artifact, execution.clone())
        .expect("foreign artifact target");
    let foreign_before = foreign.snapshot().expect("foreign before");
    assert!(
        foreign
            .restore_snapshot(&checkpoint)
            .expect_err("foreign artifact rejection")
            .contains("cartridge identity mismatch")
    );
    assert_eq!(
        foreign.snapshot().expect("foreign unchanged"),
        foreign_before
    );

    let mut other_request = execution.clone();
    other_request.config.seed += 1;
    let mut foreign_request = Pico8EmulatorBackend
        .create_execution(&artifact, other_request)
        .expect("foreign request target");
    assert!(
        foreign_request
            .restore_snapshot(&checkpoint)
            .expect_err("foreign request rejection")
            .contains("execution-request identity mismatch")
    );

    let mut target = Pico8EmulatorBackend
        .create_execution(&artifact, execution)
        .expect("transaction target");
    let before = target.snapshot().expect("target before");

    let mut noncanonical = checkpoint.clone();
    noncanonical.push(b'\n');
    assert!(
        target
            .restore_snapshot(&noncanonical)
            .expect_err("noncanonical rejection")
            .contains("not canonical JSON")
    );
    assert_eq!(
        target.snapshot().expect("unchanged after noncanonical"),
        before
    );

    let mut tampered: Value = serde_json::from_slice(&checkpoint).expect("continuation JSON");
    let byte = tampered["runtime"]["ram"][0].as_u64().expect("RAM byte") as u8;
    tampered["runtime"]["ram"][0] = json!(byte ^ 0xff);
    let tampered = canonical_json_bytes(&tampered).expect("canonical tamper");
    assert!(
        target
            .restore_snapshot(&tampered)
            .expect_err("state tamper rejection")
            .contains("integrity digest mismatch")
    );
    assert_eq!(target.snapshot().expect("unchanged after tamper"), before);

    let mut replay_tamper: Value = serde_json::from_slice(&checkpoint).expect("continuation JSON");
    let byte = replay_tamper["runtime"]["ram"][0]
        .as_u64()
        .expect("RAM byte") as u8;
    replay_tamper["runtime"]["ram"][0] = json!(byte ^ 0xff);
    resign_continuation(&mut replay_tamper);
    let replay_tamper = canonical_json_bytes(&replay_tamper).expect("canonical replay tamper");
    assert!(
        target
            .restore_snapshot(&replay_tamper)
            .expect_err("replay state rejection")
            .contains("does not match deterministic cartridge replay")
    );
    assert_eq!(
        target.snapshot().expect("unchanged after replay tamper"),
        before
    );

    let mut bad_cursor: Value = serde_json::from_slice(&checkpoint).expect("continuation JSON");
    bad_cursor["applied_stimuli"] = json!(0);
    resign_continuation(&mut bad_cursor);
    let bad_cursor = canonical_json_bytes(&bad_cursor).expect("canonical cursor tamper");
    assert!(
        target
            .restore_snapshot(&bad_cursor)
            .expect_err("cursor rejection")
            .contains("leaves past stimulus")
    );
    assert_eq!(target.snapshot().expect("unchanged after cursor"), before);

    let mut unknown: Value = serde_json::from_slice(&checkpoint).expect("continuation JSON");
    unknown["parasite"] = json!(true);
    let unknown = canonical_json_bytes(&unknown).expect("canonical unknown field");
    assert!(
        target
            .restore_snapshot(&unknown)
            .expect_err("unknown field rejection")
            .contains("unknown field")
    );
    assert_eq!(target.snapshot().expect("unchanged after unknown"), before);
}
