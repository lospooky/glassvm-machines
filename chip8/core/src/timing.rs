//! Native CHIP-8 timing defaults.

/// Nominal timer and display refresh frequency.
pub const TIMER_HERTZ: u32 = 60;

/// Default interpreter cycles executed per timer tick.
pub const DEFAULT_CYCLES_PER_FRAME: u32 = 12;
