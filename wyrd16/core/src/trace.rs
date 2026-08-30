//! Native step-trace representation.

use crate::StepEffect;

/// One exact native transition paired with its monotonically increasing cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeStepTrace {
    pub cycle: u64,
    pub effect: StepEffect,
}
