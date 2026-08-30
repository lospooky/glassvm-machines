//! Native machine snapshots, independent of GlassVM sessions.

use serde::{Deserialize, Serialize};

use crate::{CoreError, MachineState};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSnapshot {
    pub state: MachineState,
}

impl NativeSnapshot {
    pub fn new(state: &MachineState) -> Self {
        Self {
            state: state.clone(),
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, CoreError> {
        serde_json::to_vec(self).map_err(|error| CoreError::new(error.to_string()))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CoreError> {
        serde_json::from_slice(bytes).map_err(|error| CoreError::new(error.to_string()))
    }
}
