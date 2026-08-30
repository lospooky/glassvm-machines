//! Hexwell: Execution-session orchestration and lifecycle control.

use glassvm_core::{
    Emission, EmissionSink, EmulatorSession, EventKind, ExecutionRequest, NormalizerDriver,
    PreparedObservation, RunResult, SnapshotCapture,
};

use crate::emulator_backend::ScheduledTideInput;
use crate::identity::SESSION_SNAPSHOT_FORMAT_VERSION;
use crate::session_snapshot::{
    ContinuationContract, SessionImage, SessionLifecycle, validate_session_image,
};
use hexwell_core::SweepOutcome;

pub(crate) struct HexwellSession {
    pub(crate) image: SessionImage,
    pub(crate) initial: SessionImage,
    pub(crate) request: ExecutionRequest,
    pub(crate) started: bool,
    pub(crate) failed: bool,
    pub(crate) prepared_observation: PreparedObservation,
    pub(crate) max_frames: u64,
    pub(crate) sweeps_per_frame: u32,
    pub(crate) scheduled_inputs: Vec<ScheduledTideInput>,
    pub(crate) initial_scheduled_inputs: Vec<ScheduledTideInput>,
}

impl HexwellSession {
    fn perform_sweep(&mut self) -> Result<SweepOutcome, String> {
        let mut reactor = self.image.reactor.clone();
        let outcome = reactor.sweep()?;
        let mut telemetry = self.image.telemetry.clone();
        telemetry.absorb_sweep(&outcome)?;
        self.image.reactor = reactor;
        self.image.telemetry = telemetry;
        Ok(outcome)
    }

    fn advance_frame_silent(&mut self) -> Result<(), String> {
        if !self.image.frame_open {
            if self.image.reactor.quenched() {
                return Ok(());
            }
            let mut ignored_sequence = 0;
            self.apply_scheduled_inputs(&mut ignored_sequence, None)?;
            self.image.frame_open = true;
            self.image.sweeps_into_frame = 0;
            self.apply_feed(&mut ignored_sequence, None)?;
        }

        while self.image.sweeps_into_frame < self.sweeps_per_frame {
            if self.image.reactor.quenched() {
                break;
            }
            self.perform_sweep()?;
            self.image.sweeps_into_frame += 1;
        }
        self.image.reactor.finish_frame()?;
        self.image.frame_open = false;
        self.image.sweeps_into_frame = 0;
        self.capture_frame();
        Ok(())
    }
}

impl EmulatorSession for HexwellSession {
    fn request(&self) -> &ExecutionRequest {
        &self.request
    }

    fn execute(&mut self, sink: &mut dyn EmissionSink) -> Result<RunResult, String> {
        if self.failed {
            return Err("Hexwell session failed; reset before executing again".into());
        }
        if self.started {
            return Err(
                "Hexwell session has already run; call reset before executing again".into(),
            );
        }
        let result = (|| {
            self.started = true;
            self.image.lifecycle = SessionLifecycle::Terminal;
            let mut normalizer_driver = NormalizerDriver::new_with_prepared_observation(
                crate::normalizer::HexwellNormalizer::new(),
                sink,
                &self.prepared_observation,
            );
            let sink: &mut dyn EmissionSink = &mut normalizer_driver;
            sink.emit(Emission::RunStarted(&self.request))
                .map_err(|error| error.to_string())?;
            let mut sequence = 0;
            self.emit_lifecycle(EventKind::RunStarted, &mut sequence, sink)?;

            let mut terminal_periodic_snapshot_due = false;
            let mut final_state = None;
            for frame_index in 0..self.max_frames {
                self.apply_scheduled_inputs(&mut sequence, Some(sink))?;
                self.image.frame_open = true;
                self.image.sweeps_into_frame = 0;
                self.apply_feed(&mut sequence, Some(sink))?;
                while self.image.sweeps_into_frame < self.sweeps_per_frame {
                    if self.image.reactor.quenched() {
                        break;
                    }
                    let outcome = self.perform_sweep()?;
                    self.image.sweeps_into_frame += 1;
                    self.emit_sweep(&outcome, &mut sequence, sink)?;
                    if let SnapshotCapture::EverySteps(interval) =
                        self.request.observation.snapshots.capture
                        && self.image.reactor.sweeps.is_multiple_of(interval)
                    {
                        let frame_is_complete = self.image.reactor.quenched()
                            || self.image.sweeps_into_frame == self.sweeps_per_frame;
                        let run_is_complete = frame_is_complete
                            && (self.image.reactor.quenched()
                                || frame_index + 1 == self.max_frames);
                        if run_is_complete {
                            terminal_periodic_snapshot_due = true;
                        } else {
                            self.emit_snapshot(sequence, SessionLifecycle::Incremental, sink)?;
                        }
                    }
                }
                let cooling = self.image.reactor.finish_frame()?;
                self.image.frame_open = false;
                self.image.sweeps_into_frame = 0;
                let frame_hash = self.capture_frame();
                self.emit_frame(&cooling, frame_hash, &mut sequence, sink)?;
                if self.image.reactor.quenched() {
                    break;
                }
            }

            if terminal_periodic_snapshot_due
                || matches!(
                    self.request.observation.snapshots.capture,
                    SnapshotCapture::Final
                )
            {
                final_state =
                    Some(self.emit_snapshot(sequence, SessionLifecycle::Terminal, sink)?);
            }
            self.emit_lifecycle(EventKind::RunHalted, &mut sequence, sink)?;
            let termination = if self.image.reactor.quenched() {
                "quenched"
            } else {
                "frame_budget"
            };
            if !self
                .prepared_observation
                .native_observation_schemas
                .is_empty()
            {
                for observation in self.observations(termination) {
                    sink.emit(Emission::NativeEvidence(&observation))
                        .map_err(|error| error.to_string())?;
                }
            }
            let normalizer_run = normalizer_driver
                .finish(final_state.as_ref())
                .map_err(|error| error.to_string())?;
            let (mut downstream, normalizer_output) = normalizer_run.into_parts();
            let normalized = crate::replay::normalized_capabilities(&normalizer_output);
            let result = self.result(termination, &normalized)?;
            let sink: &mut dyn EmissionSink = &mut downstream;
            sink.emit(Emission::RunFinished(&result))
                .map_err(|error| error.to_string())?;
            Ok(result)
        })();
        if result.is_err() {
            self.failed = true;
            self.image.lifecycle = SessionLifecycle::Terminal;
        }
        result
    }

    fn step_frame(&mut self) -> Result<(), String> {
        if self.failed || self.image.lifecycle == SessionLifecycle::Terminal {
            return Err("Hexwell session is terminal; reset before stepping".into());
        }
        self.started = true;
        self.image.lifecycle = SessionLifecycle::Incremental;
        let result = self.advance_frame_silent();
        if result.is_err() {
            self.failed = true;
            self.image.lifecycle = SessionLifecycle::Terminal;
        }
        result
    }

    fn reset(&mut self) -> Result<(), String> {
        self.image = self.initial.clone();
        self.scheduled_inputs = self.initial_scheduled_inputs.clone();
        self.started = false;
        self.failed = false;
        Ok(())
    }

    fn snapshot(&self) -> Result<Vec<u8>, String> {
        if self.failed {
            return Err("cannot snapshot a failed Hexwell session; reset first".into());
        }
        self.snapshot_bytes()
    }

    fn restore_snapshot(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.failed || self.image.lifecycle == SessionLifecycle::Terminal {
            return Err("Hexwell session is terminal; reset before restoring".into());
        }
        let preflight: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid Hexwell snapshot: {error}"))?;
        let version = preflight
            .get("snapshot_version")
            .and_then(serde_json::Value::as_u64);
        if version != Some(u64::from(SESSION_SNAPSHOT_FORMAT_VERSION)) {
            return Err(format!(
                "unsupported Hexwell snapshot version {:?}; restart the run with snapshot format v{}",
                version, SESSION_SNAPSHOT_FORMAT_VERSION
            ));
        }
        let restored: SessionImage =
            serde_json::from_slice(bytes).map_err(|error| format!("invalid snapshot: {error}"))?;
        if restored.contract != ContinuationContract::from_request(&self.request) {
            return Err(
                "snapshot execution contract does not match this frozen Hexwell session".into(),
            );
        }
        restored.reactor.validate()?;
        if restored.reactor.plate != self.initial.reactor.plate {
            return Err("snapshot catalyst plate does not match this Hexwell ROM".into());
        }
        validate_session_image(
            &restored,
            &self.scheduled_inputs,
            self.max_frames,
            self.sweeps_per_frame,
        )?;
        if restored.lifecycle == SessionLifecycle::Fresh && restored != self.initial {
            return Err("fresh Hexwell snapshot is not canonical boot".into());
        }
        if self.image.lifecycle == SessionLifecycle::Incremental
            && (restored.lifecycle == SessionLifecycle::Fresh
                || restored.reactor.sweeps <= self.image.reactor.sweeps)
        {
            return Err("Hexwell snapshot would rewind or repeat current progress".into());
        }
        self.image = restored;
        self.started = self.image.lifecycle != SessionLifecycle::Fresh;
        self.failed = false;
        Ok(())
    }
}
