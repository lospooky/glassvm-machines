//! Native sweep execution facade.

use crate::{CoreError, ReactorState, SweepOutcome};

pub fn execute_sweep(state: &mut ReactorState) -> Result<SweepOutcome, CoreError> {
    state.sweep().map_err(CoreError::new)
}
