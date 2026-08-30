//! Native sweep trace independent of GlassVM schemas.

use crate::SweepOutcome;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSweepTrace {
    pub sweep: u64,
    pub outcome: SweepOutcome,
}
