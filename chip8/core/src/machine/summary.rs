use crate::coverage::CoverageSummary;

/// Reason execution of a ROM stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminationReason {
    /// `00FD` halt instruction reached.
    Completed,
    /// An unknown / unimplemented opcode was encountered.
    InvalidOpcode,
    /// `00EE` (RET) executed with an empty stack.
    StackUnderflow,
    /// `2NNN` (CALL) executed with a full stack (> 16 levels).
    StackOverflow,
    /// Ran for the configured maximum without halting.
    Timeout,
    /// Blocked on `FX0A` (key wait) with no more input available.
    WaitingForInput,
    /// A memory instruction computed an address outside the 64 kB space.
    MemoryFault,
}

/// A summary of a completed execution run.
///
/// Produced by [`crate::emulator::Engine`] at the end of `run_cycles`,
/// `run_frames`, or `run_with_input_script`.
#[derive(Debug, Clone)]
pub struct ExecutionSummary {
    /// FNV-1a hash of the ROM bytes loaded at construction time.
    pub rom_hash: u64,
    /// Total instructions executed.
    pub cycles: u64,
    /// Total 60 Hz frames stepped.
    pub frames: u64,
    /// True if at least one instruction completed without an immediate error.
    pub boot_success: bool,
    /// Why execution stopped.
    pub termination: TerminationReason,
    /// Number of `DXYN` draw instructions executed.
    pub draw_count: u32,
    /// Number of `DXYN` draws where VF was set to 1 (pixel collision occurred).
    pub collision_count: u32,
    /// Number of `FX0A` key-wait instructions entered.
    pub input_opcode_count: u32,
    /// Number of distinct framebuffer states observed across all frames.
    pub unique_frame_count: u32,
    /// Coverage statistics (populated in Phase 2; zeroed in Phase 1).
    pub coverage: CoverageSummary,
    /// Fraction of frames identical to a previous frame (0.0 – 1.0).
    /// 1.0 = screen never changed; 0.0 = every frame was unique.
    pub stagnation_score: f32,
    /// Reserved for Tier-3 interestingness computation; always 0.0 in Phase 1.
    pub interestingness_score: f32,
}
