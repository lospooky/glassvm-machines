use std::sync::{Arc, Mutex};

use glassvm_core::{
    InputCoordinate, InputId, LiveInputAcceptance, LiveInputAdmission, LiveInputCapability,
    LiveInputController, LiveInputError, PreparedRun, StructuredValue, TypedInputPayload,
};

use crate::emulator_backend::Chip8EmulatorBackend;

pub(crate) type SharedChip8LiveInput = Arc<Mutex<Chip8LiveInputState>>;

#[derive(Debug, Default)]
pub(crate) struct Chip8LiveInputState {
    pub key_mask: u16,
    pub current_frame: u64,
}

pub(crate) struct Chip8LiveInputController {
    admission: LiveInputAdmission,
    state: SharedChip8LiveInput,
}

impl LiveInputCapability for Chip8EmulatorBackend {
    fn create_controller(
        &self,
        prepared: &PreparedRun,
    ) -> Result<Box<dyn LiveInputController>, String> {
        let state = Arc::new(Mutex::new(Chip8LiveInputState::default()));
        self.register_live_input_state(prepared.run_id.clone(), state.clone());
        Ok(Box::new(Chip8LiveInputController {
            admission: LiveInputAdmission::new(prepared)?,
            state,
        }))
    }
}

impl LiveInputController for Chip8LiveInputController {
    fn apply(
        &mut self,
        input_id: InputId,
        payload: TypedInputPayload,
    ) -> Result<LiveInputAcceptance, LiveInputError> {
        self.admission
            .validate_before_application(&input_id, &payload)?;
        let key = input_id
            .as_str()
            .strip_prefix("chip8.key.")
            .and_then(|value| u8::from_str_radix(value, 16).ok())
            .filter(|key| *key < 16)
            .ok_or_else(|| LiveInputError::ApplicationRejected {
                input_id: input_id.clone(),
                reason: "unsupported CHIP-8 key input".into(),
            })?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| LiveInputError::ApplicationRejected {
                input_id: input_id.clone(),
                reason: "CHIP-8 live-input state is unavailable".into(),
            })?;
        let active = match payload.value {
            StructuredValue::Bool(active) => active,
            _ => {
                return Err(LiveInputError::InvalidPayload {
                    input_id,
                    reason: "CHIP-8 key payload must be boolean".into(),
                });
            }
        };
        let payload_for_acceptance = TypedInputPayload {
            schema: payload.schema,
            value: StructuredValue::Bool(active),
        };
        if active {
            state.key_mask |= 1_u16 << key;
        } else {
            state.key_mask &= !(1_u16 << key);
        }
        self.admission.record_application_acceptance(
            &input_id,
            &payload_for_acceptance,
            InputCoordinate::frame(state.current_frame),
        )
    }
}

impl Chip8EmulatorBackend {
    pub(crate) fn register_live_input_state(
        &self,
        run_id: glassvm_core::RunId,
        state: SharedChip8LiveInput,
    ) {
        if let Ok(mut states) = self.live_inputs.lock() {
            states.insert(run_id, state);
        }
    }

    pub(crate) fn take_live_input_state(
        &self,
        run_id: &glassvm_core::RunId,
    ) -> Option<SharedChip8LiveInput> {
        self.live_inputs.lock().ok()?.remove(run_id)
    }
}
