/// A run-termination policy for [`crate::emulator::Engine::run_with_policy`].
///
/// Policies compose a _limit_ (when to stop) with optional _early-exit
/// conditions_ (what counts as an interesting or degenerate result).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RunPolicy {
    /// Run for exactly N 60 Hz frames.
    Frames(u64),
    /// Run for exactly N instruction cycles (ignores frame boundaries).
    Cycles(u64),
    /// Run until `00FD` (SUPER-CHIP EXIT) or an error, with a frame cap
    /// as a safety limit.
    UntilHalt { max_frames: u64 },
    /// Run until the proportion of unique frames over the last `window`
    /// frames falls below `threshold` — the program appears to have stalled
    /// or entered a tight loop.
    ///
    /// Also stops at `max_frames`.
    UntilStagnant {
        max_frames: u64,
        /// Rolling window length in frames.
        window: u64,
        /// Stop when `unique_in_window / window < threshold`.
        threshold: f32,
    },
}

impl RunPolicy {
    /// Convenience: run for N frames (the most common case).
    pub fn frames(n: u64) -> Self {
        Self::Frames(n)
    }

    pub fn cycles(n: u64) -> Self {
        Self::Cycles(n)
    }

    pub fn until_halt(max_frames: u64) -> Self {
        Self::UntilHalt { max_frames }
    }

    pub fn until_stagnant(max_frames: u64, window: u64, threshold: f32) -> Self {
        Self::UntilStagnant {
            max_frames,
            window,
            threshold,
        }
    }
}
