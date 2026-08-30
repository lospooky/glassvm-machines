//! Reusable native Hexwell emulator facade.

use crate::{
    Artifact, CoreError, MachineConfiguration, NativeSnapshot, ReactorState, SweepOutcome,
    Telemetry, TideInput,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emulator {
    state: ReactorState,
    initial: ReactorState,
    telemetry: Telemetry,
    initial_telemetry: Telemetry,
}

impl Emulator {
    pub fn new(bytes: &[u8], configuration: MachineConfiguration) -> Result<Self, CoreError> {
        let artifact = Artifact::parse(bytes)?;
        let state =
            ReactorState::boot(artifact.bytes(), configuration.seed).map_err(CoreError::new)?;
        let telemetry = Telemetry::boot(state.total_matter());
        Ok(Self {
            initial: state.clone(),
            state,
            initial_telemetry: telemetry.clone(),
            telemetry,
        })
    }

    pub fn state(&self) -> &ReactorState {
        &self.state
    }

    pub fn telemetry(&self) -> &Telemetry {
        &self.telemetry
    }

    pub fn set_input(&mut self, input: TideInput) {
        self.state.set_tide(input.mask());
    }

    pub fn sweep(&mut self) -> Result<SweepOutcome, CoreError> {
        let outcome = self.state.sweep().map_err(CoreError::new)?;
        self.telemetry
            .absorb_sweep(&outcome)
            .map_err(CoreError::new)?;
        Ok(outcome)
    }

    pub fn snapshot(&self) -> NativeSnapshot {
        NativeSnapshot {
            state: self.state.clone(),
            telemetry: self.telemetry.clone(),
        }
    }

    pub fn restore(&mut self, snapshot: &NativeSnapshot) {
        self.state = snapshot.state.clone();
        self.telemetry = snapshot.telemetry.clone();
    }

    pub fn reset(&mut self) {
        self.state = self.initial.clone();
        self.telemetry = self.initial_telemetry.clone();
    }
}
