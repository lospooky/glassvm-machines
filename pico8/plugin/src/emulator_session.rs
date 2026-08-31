use glassvm_core::{
    CommonMetrics, Emission, EmissionSink, EmulatorSession, ExecutionRequest, InputCoordinate,
    PreparedObservation, PreparedRun, RunResult, SinkError, StructuredValue,
};
use pico8_core::{Cartridge, Pico8Runtime, RuntimeSnapshot};

use crate::emulator_backend::configuration_values;

struct ScheduledInput {
    frame: u64,
    bit: u16,
    active: bool,
}

pub(super) struct Pico8Session {
    request: ExecutionRequest,
    artifact: Vec<u8>,
    runtime: Pico8Runtime,
    scheduled_inputs: Vec<ScheduledInput>,
    next_input: usize,
    cycles_per_frame: u32,
    instruction_budget: u64,
    machine_seed: u64,
    prepared_observation: PreparedObservation,
    terminal: bool,
}

impl Pico8Session {
    pub(super) fn new(
        artifact: &[u8],
        request: ExecutionRequest,
        prepared_run: PreparedRun,
        prepared_observation: PreparedObservation,
    ) -> Result<Self, String> {
        let (cycles_per_frame, instruction_budget, machine_seed) =
            configuration_values(&prepared_run.configuration)?;
        let cartridge = Cartridge::parse(artifact)
            .map_err(|error| format!("invalid PICO-8 cartridge: {error}"))?;
        let runtime = Pico8Runtime::new(&cartridge, machine_seed, instruction_budget)?;
        let scheduled_inputs = scheduled_inputs(&prepared_run)?;
        Ok(Self {
            request,
            artifact: artifact.to_vec(),
            runtime,
            scheduled_inputs,
            next_input: 0,
            cycles_per_frame,
            instruction_budget,
            machine_seed,
            prepared_observation,
            terminal: false,
        })
    }

    fn apply_inputs(&mut self, frame: u64) -> Result<(), String> {
        while let Some(input) = self.scheduled_inputs.get(self.next_input) {
            if input.frame > frame {
                break;
            }
            if input.frame < frame {
                return Err(format!(
                    "PICO-8 scheduled input for frame {} was not applied before frame {frame}",
                    input.frame
                ));
            }
            let mut mask = self.runtime.snapshot().input_mask;
            if input.active {
                mask |= input.bit;
            } else {
                mask &= !input.bit;
            }
            self.runtime.set_input_mask(mask);
            self.next_input += 1;
        }
        Ok(())
    }

    fn step_native_frame(&mut self) -> Result<(), String> {
        let frame = self.runtime.frame();
        self.apply_inputs(frame)?;
        self.runtime.step_frame()
    }

    fn frame_limit(&self) -> Result<u64, String> {
        self.request
            .execution_controls
            .frame_limit
            .ok_or_else(|| "PICO-8 execution requires an explicit frame_limit".into())
    }

    fn reset_runtime(&mut self) -> Result<(), String> {
        let cartridge = Cartridge::parse(&self.artifact)
            .map_err(|error| format!("invalid PICO-8 cartridge: {error}"))?;
        self.runtime = Pico8Runtime::new(&cartridge, self.machine_seed, self.instruction_budget)?;
        self.next_input = 0;
        self.terminal = false;
        Ok(())
    }
}

impl EmulatorSession for Pico8Session {
    fn request(&self) -> &ExecutionRequest {
        &self.request
    }

    fn execute(&mut self, sink: &mut dyn EmissionSink) -> Result<RunResult, String> {
        if self.terminal {
            return Err("PICO-8 session is terminal; reset before execute".into());
        }
        self.prepared_observation.validate()?;
        let frame_limit = self.frame_limit()?;
        sink.emit(Emission::RunStarted(&self.request))
            .map_err(sink_error)?;
        let mut runtime_error = None;
        while self.runtime.frame() < frame_limit {
            if let Err(error) = self.step_native_frame() {
                runtime_error = Some(error);
                break;
            }
        }
        self.terminal = true;
        let frames = self.runtime.frame();
        let result = RunResult {
            common: CommonMetrics {
                cycles: frames.saturating_mul(u64::from(self.cycles_per_frame)),
                frames,
                termination: if runtime_error.is_some() {
                    "runtime_error".into()
                } else {
                    "frame_limit".into()
                },
                boot_success: true,
            },
            capabilities: Vec::new(),
        };
        sink.emit(Emission::RunFinished(&result))
            .map_err(sink_error)?;
        if let Some(error) = runtime_error {
            return Err(error);
        }
        Ok(result)
    }

    fn step_frame(&mut self) -> Result<(), String> {
        if self.terminal {
            return Err("PICO-8 session is terminal; reset before stepping".into());
        }
        self.step_native_frame()
    }

    fn reset(&mut self) -> Result<(), String> {
        self.reset_runtime()
    }

    fn snapshot(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&self.runtime.snapshot()).map_err(|error| error.to_string())
    }

    fn restore_snapshot(&mut self, bytes: &[u8]) -> Result<(), String> {
        let _: RuntimeSnapshot = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid PICO-8 snapshot: {error}"))?;
        Err("PICO-8 snapshot restoration is part of the next clean snapshot slice".into())
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
                        "PICO-8 input {} must use a frame coordinate",
                        entry.input_id
                    ));
                }
            };
            let button = entry
                .input_id
                .as_str()
                .rsplit('.')
                .next()
                .ok_or_else(|| format!("invalid PICO-8 input ID {}", entry.input_id))?
                .chars()
                .next()
                .and_then(|digit| digit.to_digit(16))
                .ok_or_else(|| format!("invalid PICO-8 button input ID {}", entry.input_id))?;
            let active = match &entry.payload.value {
                StructuredValue::Bool(value) => *value,
                StructuredValue::Unsigned(value) => match value {
                    0 => false,
                    1 => true,
                    _ => return Err("PICO-8 button payload must be boolean or 0/1".into()),
                },
                StructuredValue::Signed(value) => match value {
                    0 => false,
                    1 => true,
                    _ => return Err("PICO-8 button payload must be boolean or 0/1".into()),
                },
                _ => return Err("PICO-8 button payload must be boolean or 0/1".into()),
            };
            Ok(ScheduledInput {
                frame,
                bit: 1_u16 << button,
                active,
            })
        })
        .collect()
}

fn sink_error(error: SinkError) -> String {
    error.to_string()
}
