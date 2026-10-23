//! Finite caller-owned test projections, not an execution retention model.
use std::collections::BTreeSet;

use glassvm_core::{
    BudgetedSink, Emission, EmissionChannel, EmissionSink, EmulatorSession, EventKind,
    EventSelection, EvidenceStatus, ExecutionControls, ExecutionEvent, ExecutionRequest,
    FrameArtifact, FrameCapture, InputSchedule, MachineBundle, MachineConfiguration,
    ObservationRequest, OverflowPolicy, RunResult, SchemaRef, SchemaVersion, SinkError,
};

const GRAPHICS: &str = "glassvm.graphics_activity";

fn cases() -> Vec<(Box<dyn MachineBundle>, Vec<u8>, u64)> {
    let pico = b"pico-8 cartridge // http://www.pico-8.com\nversion 41\n__lua__\n\
        function _init() cls() end\n\
        n=0\nfunction _draw()\nn=n+1\n\
          if n==2 then return end\n\
          cls() pset(-100,-100,1) rectfill(0,0,0,0,2) rect(0,0,1,1,3)\n\
          line(0,0,1,1,4) spr(0,-100,-100) sspr(0,0,0,0,0,0)\n\
          local read=pget(0,0)\n\
        end\n"
        .to_vec();
    let source = "cls() n=0\nfunction TIC() n=n+1 if n==2 then return end\n\
        cls() pix(-100,-100,1) line(0,0,1,1,2) rect(0,0,0,0,3) rectb(0,0,1,1,4)\n\
        circ(0,0,0,5) circb(0,0,0,6) tri(0,0,0,0,0,0,7) trib(0,0,0,0,0,0,8)\n\
        ttri(0,0,0,0,0,0,0,0,0,0,0,0) spr(0,-100,-100) map(0,0,0,0)\n\
        local read=pix(0,0)\nend";
    let mut tic = vec![5];
    tic.extend_from_slice(&(source.len() as u16).to_le_bytes());
    tic.push(0);
    tic.extend_from_slice(source.as_bytes());
    vec![
        (Box::new(pico8_bundle::Pico8Plugin::new()), pico, 7),
        (Box::new(tic80_bundle::Tic80Plugin::new()), tic, 12),
    ]
}

#[derive(Default)]
struct Projection {
    events: Vec<ExecutionEvent>,
    frames: Vec<FrameArtifact>,
    native_count: usize,
}

impl EmissionSink for Projection {
    fn emit(&mut self, emission: Emission<'_>) -> Result<(), SinkError> {
        match emission {
            Emission::Event(event) => self.events.push(event.clone()),
            Emission::Frame(frame) => self.frames.push(frame.clone()),
            Emission::NativeEvidence(_) => self.native_count += 1,
            _ => {}
        }
        Ok(())
    }
}

fn observation(events: EventSelection, frames: FrameCapture) -> ObservationRequest {
    let mut observation = ObservationRequest::summary();
    observation.normalized_events.events = events;
    observation.frames.capture = frames;
    observation
}

fn graphics_only() -> EventSelection {
    EventSelection::Kinds(BTreeSet::from([EventKind::Extension(GRAPHICS.into())]))
}

fn session(
    bundle: &dyn MachineBundle,
    artifact: &[u8],
    observation: ObservationRequest,
) -> (ExecutionRequest, Box<dyn EmulatorSession>) {
    let mut request = ExecutionRequest::new(
        "graphics-contract",
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
    let prepared_run = bundle.prepare_run(&request).unwrap();
    let session = bundle
        .emulator()
        .create_execution_with_prepared_run(
            artifact,
            request.clone(),
            prepared_run,
            prepared_observation,
        )
        .unwrap();
    (request, session)
}

fn execute(
    bundle: &dyn MachineBundle,
    artifact: &[u8],
    events: EventSelection,
    frames: FrameCapture,
) -> (RunResult, Projection) {
    let (_, mut session) = session(bundle, artifact, observation(events, frames));
    let mut projection = Projection::default();
    let result = session.execute(&mut projection).unwrap();
    (result, projection)
}

#[test]
fn shared_schema_counts_api_calls_not_pixels_or_initialization_or_reads() {
    assert_eq!(pico8_bundle::GRAPHICS_ACTIVITY, GRAPHICS);
    assert_eq!(tic80_bundle::GRAPHICS_ACTIVITY, GRAPHICS);
    for (bundle, artifact, calls) in cases() {
        let (result, projection) =
            execute(&*bundle, &artifact, graphics_only(), FrameCapture::None);
        assert!(result.common.boot_success);
        assert_eq!(projection.events.len(), 2, "no empty-frame filler");
        assert!(projection.frames.is_empty());
        assert_eq!(projection.native_count, 0);
        for (event, frame) in projection.events.iter().zip([0, 2]) {
            assert_eq!(event.kind, EventKind::Extension(GRAPHICS.into()));
            assert_eq!(event.frame, Some(frame));
            assert_eq!(event.step, frame);
            assert_eq!(
                event.extensions[GRAPHICS],
                serde_json::json!({
                    "schema": SchemaRef::new(GRAPHICS, SchemaVersion::V1),
                    "draw_calls": calls,
                })
            );
        }
    }
}

#[test]
fn selectors_are_independent_from_frame_completion_and_frame_artifacts() {
    for (bundle, artifact, _) in cases() {
        let (_, omitted) = execute(
            &*bundle,
            &artifact,
            EventSelection::Kinds(BTreeSet::from([EventKind::FrameCompleted])),
            FrameCapture::None,
        );
        assert_eq!(omitted.events.len(), 3);
        assert!(
            omitted
                .events
                .iter()
                .all(|event| event.kind == EventKind::FrameCompleted)
        );
        for frames in [FrameCapture::Hashes, FrameCapture::Full] {
            let (_, with) = execute(&*bundle, &artifact, EventSelection::All, frames);
            let (_, without) = execute(&*bundle, &artifact, EventSelection::None, frames);
            assert_eq!(with.frames.len(), 3);
            assert_eq!(with.frames.len(), without.frames.len());
            for (with, without) in with.frames.iter().zip(&without.frames) {
                // Sequence is not semantic frame identity or a cross-channel
                // merge key; changing selected channels changes its allocation.
                assert_eq!(with.frame, without.frame);
                assert_eq!(with.step, without.step);
                assert_eq!(with.representation_schema, without.representation_schema);
                assert_eq!(
                    with.evidence, without.evidence,
                    "observation must not alter machine pixels"
                );
            }
            assert_eq!(
                with.events
                    .iter()
                    .filter(|e| e.kind == EventKind::Extension(GRAPHICS.into()))
                    .count(),
                2
            );
            assert!(without.events.is_empty());
            for event in with
                .events
                .iter()
                .filter(|e| e.kind == EventKind::Extension(GRAPHICS.into()))
            {
                let boundary = with
                    .events
                    .iter()
                    .find(|other| {
                        other.kind == EventKind::FrameCompleted && other.frame == event.frame
                    })
                    .unwrap();
                assert_eq!(event.step, boundary.step);
            }
        }
    }
}

#[test]
fn continuation_does_not_retain_or_recount_historical_graphics() {
    for (bundle, artifact, calls) in cases() {
        let (_, mut paused) = session(
            &*bundle,
            &artifact,
            observation(graphics_only(), FrameCapture::None),
        );
        paused.step_frame().unwrap();
        let state = paused.snapshot().unwrap();
        let (_, mut resumed) = session(
            &*bundle,
            &artifact,
            observation(graphics_only(), FrameCapture::None),
        );
        resumed.restore_snapshot(&state).unwrap();
        let mut projection = Projection::default();
        resumed.execute(&mut projection).unwrap();
        assert_eq!(projection.events.len(), 1);
        assert_eq!(projection.events[0].frame, Some(2));
        assert_eq!(
            projection.events[0].extensions[GRAPHICS]["draw_calls"],
            calls
        );
    }
}

#[test]
fn graphics_uses_normalized_budget_without_redefining_machine_success() {
    for (bundle, artifact, _) in cases() {
        let mut observation = observation(graphics_only(), FrameCapture::None);
        observation.budgets.max_normalized_events = Some(1);
        observation.overflow = OverflowPolicy::AllowIncomplete;
        let (request, mut session) = session(&*bundle, &artifact, observation);
        let mut sink = BudgetedSink::new(
            Projection::default(),
            request.observation,
            request.run_id,
            request.machine_id,
            bundle.descriptor().machine_version.clone(),
        )
        .unwrap();
        let result = session.execute(&mut sink).unwrap();
        let receipt = sink.emit_receipt().unwrap();
        assert!(result.common.boot_success);
        assert_eq!(result.common.frames, 3);
        assert_eq!(receipt.status, EvidenceStatus::Incomplete);
        let channel = receipt
            .channels
            .iter()
            .find(|channel| channel.channel == EmissionChannel::NormalizedEvents)
            .unwrap();
        assert_eq!(channel.item_count, 1);
        assert_eq!(sink.into_inner().events.len(), 1);
    }
}
