use glassvm_core::{
    CommonMetrics, Emission, EmissionSink, EmulatorSession, ExecutionRequest, InputCoordinate,
    PreparedObservation, PreparedRun, RunResult, SinkError, StructuredValue,
};
use tic80_core::{RuntimeSnapshot, Tic80Runtime};

use crate::emulator_backend::configuration_values;

struct ScheduledInput {
    frame: u64,
    mask: u32,
}

pub(super) struct Tic80Session {
    request: ExecutionRequest,
    artifact: Vec<u8>,
    runtime: Tic80Runtime,
    scheduled_inputs: Vec<ScheduledInput>,
    next_input: usize,
    cycles_per_frame: u32,
    machine_seed: u64,
    prepared_observation: PreparedObservation,
    terminal: bool,
}

impl Tic80Session {
    pub(super) fn new(
        artifact: &[u8],
        request: ExecutionRequest,
        prepared_run: PreparedRun,
        prepared_observation: PreparedObservation,
    ) -> Result<Self, String> {
        let (cycles_per_frame, machine_seed) = configuration_values(&prepared_run.configuration)?;
        let runtime = Tic80Runtime::new(artifact, machine_seed)?;
        let scheduled_inputs = scheduled_inputs(&prepared_run)?;
        Ok(Self {
            request,
            artifact: artifact.to_vec(),
            runtime,
            scheduled_inputs,
            next_input: 0,
            cycles_per_frame,
            machine_seed,
            prepared_observation,
            terminal: false,
        })
    }

    fn frame_limit(&self) -> Result<u64, String> {
        self.request
            .execution_controls
            .frame_limit
            .ok_or_else(|| "TIC-80 execution requires an explicit frame_limit".into())
    }

    fn input_for_frame(&mut self, frame: u64) -> Result<u32, String> {
        let mut mask = 0;
        while let Some(input) = self.scheduled_inputs.get(self.next_input) {
            if input.frame > frame {
                break;
            }
            if input.frame < frame {
                return Err(format!(
                    "TIC-80 scheduled input for frame {} was not applied before frame {frame}",
                    input.frame
                ));
            }
            mask = input.mask;
            self.next_input += 1;
        }
        Ok(mask)
    }

    fn step_native_frame(&mut self) -> Result<bool, String> {
        let frame = self.runtime.input_history().len() as u64;
        let mask = self.input_for_frame(frame)?;
        Ok(self.runtime.tick(mask)?.exit_requested)
    }

    fn reset_runtime(&mut self) -> Result<(), String> {
        self.runtime = Tic80Runtime::new(&self.artifact, self.machine_seed)?;
        self.next_input = 0;
        self.terminal = false;
        Ok(())
    }
}

impl EmulatorSession for Tic80Session {
    fn request(&self) -> &ExecutionRequest {
        &self.request
    }

    fn execute(&mut self, sink: &mut dyn EmissionSink) -> Result<RunResult, String> {
        if self.terminal {
            return Err("TIC-80 session is terminal; reset before execute".into());
        }
        self.prepared_observation.validate()?;
        let frame_limit = self.frame_limit()?;
        sink.emit(Emission::RunStarted(&self.request))
            .map_err(sink_error)?;
        let mut exit_requested = false;
        while (self.runtime.input_history().len() as u64) < frame_limit {
            exit_requested = self.step_native_frame()?;
            if exit_requested {
                break;
            }
        }
        self.terminal = true;
        let frames = self.runtime.input_history().len() as u64;
        let result = RunResult {
            common: CommonMetrics {
                cycles: frames.saturating_mul(u64::from(self.cycles_per_frame)),
                frames,
                termination: if exit_requested {
                    "machine_exit".into()
                } else {
                    "frame_limit".into()
                },
                boot_success: true,
            },
            capabilities: Vec::new(),
        };
        sink.emit(Emission::RunFinished(&result))
            .map_err(sink_error)?;
        Ok(result)
    }

    fn step_frame(&mut self) -> Result<(), String> {
        if self.terminal {
            return Err("TIC-80 session is terminal; reset before stepping".into());
        }
        self.step_native_frame().map(|_| ())
    }

    fn reset(&mut self) -> Result<(), String> {
        self.reset_runtime()
    }

    fn snapshot(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&self.runtime.snapshot()?).map_err(|error| error.to_string())
    }

    fn restore_snapshot(&mut self, bytes: &[u8]) -> Result<(), String> {
        let snapshot: RuntimeSnapshot = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid TIC-80 snapshot: {error}"))?;
        self.runtime.restore_snapshot(&snapshot)
    }
}

fn scheduled_inputs(prepared: &PreparedRun) -> Result<Vec<ScheduledInput>, String> {
    prepared
        .input_schedule
        .entries
        .iter()
        .map(|entry| {
            let frame = match entry.coordinate {
                InputCoordinate::Frame { frame } => frame,
                _ => {
                    return Err(format!(
                        "TIC-80 input {} must use a frame coordinate",
                        entry.input_id
                    ));
                }
            };
            if entry.input_id.as_str() != "tic80.gamepad" {
                return Err(format!("unsupported TIC-80 input ID {}", entry.input_id));
            }
            let mask = match &entry.payload.value {
                StructuredValue::Unsigned(value) => u32::try_from(*value).map_err(|_| ()),
                StructuredValue::Signed(value) => u32::try_from(*value).map_err(|_| ()),
                _ => Err(()),
            }
            .map_err(|_| "TIC-80 gamepad payload must be an unsigned 32-bit value".to_string())?;
            Ok(ScheduledInput { frame, mask })
        })
        .collect()
}

fn sink_error(error: SinkError) -> String {
    error.to_string()
}
