//! Reusable native emulator facade.

use crate::{
    CoreError, KeyMask, MachineConfiguration, MachineState, NativeSnapshot, StepEffect,
    execute_step,
};

/// Standalone Wyrd-16 emulator with no GlassVM dependency.
#[derive(Debug, Clone)]
pub struct Emulator {
    state: MachineState,
    initial: MachineState,
}

impl Emulator {
    pub fn new(bytes: &[u8], configuration: MachineConfiguration) -> Result<Self, CoreError> {
        let initial = MachineState::boot(bytes, configuration.seed)?;
        Ok(Self {
            state: initial.clone(),
            initial,
        })
    }

    pub fn state(&self) -> &MachineState {
        &self.state
    }

    pub fn step(&mut self) -> StepEffect {
        execute_step(&mut self.state)
    }

    pub fn set_keys(&mut self, keys: KeyMask) {
        self.state.input_mask = keys.bits();
    }

    pub fn finish_frame(&mut self) -> Result<(), CoreError> {
        self.state.frames = self
            .state
            .frames
            .checked_add(1)
            .ok_or_else(|| CoreError::new("Wyrd-16 frame counter exhausted u64"))?;
        Ok(())
    }

    pub fn snapshot(&self) -> NativeSnapshot {
        NativeSnapshot::new(&self.state)
    }

    pub fn restore(&mut self, snapshot: &NativeSnapshot) {
        self.state = snapshot.state.clone();
    }

    pub fn reset(&mut self) {
        self.state = self.initial.clone();
    }
}
