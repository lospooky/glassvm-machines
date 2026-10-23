//! Explicit finite test projections; activity production itself is constant-space.
use glassvm_core::{
    BudgetedSink, Emission, EmissionSink, EmulatorSession, EventKind, EventSelection,
    EvidenceStatus, ExecutionControls, ExecutionEvent, ExecutionRequest, FrameArtifact,
    FrameCapture, InputSchedule, MachineBundle, MachineConfiguration, ObservationRequest,
    OverflowPolicy, SchemaRef, SchemaVersion, SinkError, SnapshotCapture,
};
use std::collections::BTreeSet;

const EXECUTION: &str = "glassvm.execution_activity";
const RNG: &str = "glassvm.rng_activity";

fn cases() -> Vec<(Box<dyn MachineBundle>, Vec<u8>)> {
    let pico = b"pico-8 cartridge // http://www.pico-8.com\nversion 41\n__lua__\n\
        srand(123) local random=rnd random() n=0\n\
        function _update60() n=n+1 local sum=0 for i=1,2000 do sum=sum+i end\n\
          if n~=2 then srand(17) random() random() end end\n\
        function _draw() if n~=2 then pset(0,0,flr(random(16))) end end\n"
        .to_vec();
    let source = "math.randomseed(123) local random=math.random random() n=0
        function TIC() n=n+1 local sum=0 for i=1,2000 do sum=sum+i end
          if n~=2 then math.randomseed(17) random() random() pix(0,0,random(0,15)) end end
        function BDR(row) end";
    let mut tic = vec![5];
    tic.extend_from_slice(&(source.len() as u16).to_le_bytes());
    tic.push(0);
    tic.extend_from_slice(source.as_bytes());
    vec![
        (Box::new(pico8_bundle::Pico8Plugin::new()), pico),
        (Box::new(tic80_bundle::Tic80Plugin::new()), tic),
    ]
}

#[derive(Default)]
struct Projection {
    events: Vec<ExecutionEvent>,
    frames: Vec<FrameArtifact>,
    state: Option<serde_json::Value>,
    native: usize,
}

impl EmissionSink for Projection {
    fn emit(&mut self, emission: Emission<'_>) -> Result<(), SinkError> {
        match emission {
            Emission::Event(event) => self.events.push(event.clone()),
            Emission::Frame(frame) => self.frames.push(frame.clone()),
            Emission::Snapshot(snapshot) => {
                self.state = Some(serde_json::from_slice(&snapshot.bytes).unwrap())
            }
            Emission::NativeEvidence(_) => self.native += 1,
            _ => {}
        }
        Ok(())
    }
}

fn observation(kinds: &[&str]) -> ObservationRequest {
    let mut observation = ObservationRequest::summary();
    observation.normalized_events.events = EventSelection::Kinds(
        kinds
            .iter()
            .map(|kind| EventKind::Extension((*kind).into()))
            .collect::<BTreeSet<_>>(),
    );
    observation
}

fn session(
    bundle: &dyn MachineBundle,
    artifact: &[u8],
    observation: ObservationRequest,
) -> (ExecutionRequest, Box<dyn EmulatorSession>) {
    let mut request = ExecutionRequest::new(
        "activity-contract",
        bundle.descriptor().id.clone(),
        artifact.to_vec(),
        MachineConfiguration::defaults(&bundle.emulator().config_schema()).unwrap(),
        InputSchedule::empty(),
        observation,
        ExecutionControls {
            frame_limit: Some(3),
            ..ExecutionControls::default()
        },
    )
    .unwrap();
    let prepared_observation = bundle.prepare_observation(&request.observation).unwrap();
    request = request.with_prepared_observation_id(prepared_observation.identity);
    let prepared = bundle.prepare_run(&request).unwrap();
    let session = bundle
        .emulator()
        .create_execution_with_prepared_run(
            artifact,
            request.clone(),
            prepared,
            prepared_observation,
        )
        .unwrap();
    (request, session)
}

#[test]
fn exact_selectors_have_shared_schemas_and_only_real_runtime_activity() {
    assert_eq!(pico8_bundle::EXECUTION_ACTIVITY, EXECUTION);
    assert_eq!(tic80_bundle::RNG_ACTIVITY, RNG);
    for (bundle, artifact) in cases() {
        for kinds in [&[EXECUTION][..], &[RNG][..], &[EXECUTION, RNG][..], &[][..]] {
            let (_, mut session) = session(&*bundle, &artifact, observation(kinds));
            let mut sink = Projection::default();
            assert!(session.execute(&mut sink).unwrap().common.boot_success);
            assert_eq!(
                sink.events.len(),
                usize::from(kinds.contains(&EXECUTION)) * 3 + usize::from(kinds.contains(&RNG)) * 2
            );
            assert!(sink.frames.is_empty());
            assert!(sink.state.is_none());
            assert_eq!(sink.native, 0);
            for event in sink.events {
                assert_eq!(event.frame, Some(event.step));
                assert!(event.step < 3);
                let EventKind::Extension(id) = &event.kind else {
                    panic!("unexpected kind")
                };
                assert!(kinds.contains(&id.as_str()));
                let value = &event.extensions[id];
                assert_eq!(
                    value["schema"],
                    serde_json::to_value(SchemaRef::new(id, SchemaVersion::V1)).unwrap()
                );
                if id == RNG {
                    assert_ne!(event.frame, Some(1), "no RNG calls in this frame");
                    assert_eq!(value["random_calls"], 3);
                    assert_eq!(value["reseed_calls"], 1);
                    assert_eq!(value.as_object().unwrap().len(), 3, "no RNG values/state");
                } else {
                    let expected = if bundle.descriptor().id.as_str() == "pico8" {
                        serde_json::json!({"update":1,"draw":1,"main":0,"border":0})
                    } else {
                        serde_json::json!({"update":0,"draw":0,"main":1,"border":144})
                    };
                    assert_eq!(value["callbacks"], expected);
                    assert_eq!(value["instruction_hook"]["quantum"], 1000);
                    assert!(value["instruction_hook"]["ticks"].as_u64().unwrap() > 0);
                }
            }
        }
    }
}

#[test]
fn selected_activity_changes_neither_frame_content_nor_resumable_machine_state() {
    for (bundle, artifact) in cases() {
        let mut projections = Vec::new();
        for kinds in [&[EXECUTION, RNG][..], &[][..]] {
            let mut observation = observation(kinds);
            observation.frames.capture = FrameCapture::Full;
            observation.snapshots.capture = SnapshotCapture::Final;
            let (_, mut session) = session(&*bundle, &artifact, observation);
            let mut sink = Projection::default();
            session.execute(&mut sink).unwrap();
            projections.push(sink);
        }
        assert_eq!(
            projections[0].state.as_ref().unwrap()["machine_state"],
            projections[1].state.as_ref().unwrap()["machine_state"]
        );
        for (with, without) in projections[0].frames.iter().zip(&projections[1].frames) {
            assert_eq!((with.frame, with.step), (without.frame, without.step));
            assert_eq!(with.evidence, without.evidence);
        }
        let state = projections[0].state.as_ref().unwrap().to_string();
        for forbidden in [
            "instruction_hook",
            "random_calls",
            "reseed_calls",
            "execution_activity",
            "rng_activity",
        ] {
            assert!(!state.contains(forbidden));
        }
    }
}

#[test]
fn continuation_observes_only_new_frames_and_budget_failure_is_evidence_only() {
    for (bundle, artifact) in cases() {
        let (_, mut paused) = session(&*bundle, &artifact, observation(&[EXECUTION, RNG]));
        paused.step_frame().unwrap();
        let state = paused.snapshot().unwrap();
        let (_, mut resumed) = session(&*bundle, &artifact, observation(&[EXECUTION, RNG]));
        resumed.restore_snapshot(&state).unwrap();
        let mut projection = Projection::default();
        resumed.execute(&mut projection).unwrap();
        assert_eq!(projection.events.len(), 3);
        assert!(projection.events.iter().all(|event| event.step >= 1));
        assert_eq!(
            projection
                .events
                .iter()
                .filter(|event| event.kind == EventKind::Extension(RNG.into()))
                .count(),
            1
        );
        let mut limited = observation(&[EXECUTION, RNG]);
        limited.budgets.max_normalized_events = Some(1);
        limited.overflow = OverflowPolicy::AllowIncomplete;
        let (request, mut session) = session(&*bundle, &artifact, limited);
        let mut sink = BudgetedSink::new(
            Projection::default(),
            request.observation,
            request.run_id,
            request.machine_id,
            bundle.descriptor().machine_version.clone(),
        )
        .unwrap();
        let result = session.execute(&mut sink).unwrap();
        assert!(result.common.boot_success);
        assert_eq!(result.common.frames, 3);
        assert_eq!(
            sink.emit_receipt().unwrap().status,
            EvidenceStatus::Incomplete
        );
        assert_eq!(sink.into_inner().events.len(), 1);
    }
}
