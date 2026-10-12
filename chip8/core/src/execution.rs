use crate::configuration::QuirksConfig;
/// Batch evaluation API for running one or many ROMs with a unified config.
///
/// The entry-points are [`evaluate`] (single ROM) and [`evaluate_batch`]
/// (multiple ROMs, optionally parallel with the `parallel` feature).
use crate::emulator::{Engine, fnv1a_64};
use crate::input::InputScript;
use crate::policy::RunPolicy;
use crate::summary::ExecutionSummary;

// ── EvalConfig ────────────────────────────────────────────────────────────────

/// Full configuration for a single evaluation run.
///
/// Wraps a [`RunPolicy`] with the CPU run's seed, quirks, timing, and input
/// schedule. It does not request or retain whole-run event/frame histories.
#[derive(Debug, Clone)]
pub struct EvalConfig {
    /// How and when to stop the run.
    pub policy: RunPolicy,
    /// Initial RNG seed (used for `CXNN` random-byte instructions).
    pub seed: u64,
    /// CHIP-8 quirks mode.
    pub quirks: QuirksConfig,
    /// Instruction cycles executed per 60 Hz frame.
    pub cycles_per_frame: u32,
    /// Request per-frame hashes from a CUDA batch evaluator.
    pub capture_frame_hashes: bool,
    /// Optional scripted key input replayed frame-by-frame during evaluation.
    /// Must be sorted by ascending `frame` index.  When `None`, the ROM runs
    /// with no key input.
    pub input_script: Option<InputScript>,
}

impl Default for EvalConfig {
    fn default() -> Self {
        Self {
            policy: RunPolicy::Frames(600),
            seed: 0,
            quirks: QuirksConfig::default(),
            cycles_per_frame: 12,
            capture_frame_hashes: false,
            input_script: None,
        }
    }
}

impl EvalConfig {
    pub fn new(policy: RunPolicy) -> Self {
        Self {
            policy,
            ..Default::default()
        }
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    pub fn with_quirks(mut self, quirks: QuirksConfig) -> Self {
        self.quirks = quirks;
        self
    }

    pub fn with_cycles_per_frame(mut self, n: u32) -> Self {
        self.cycles_per_frame = n;
        self
    }

    pub fn with_frame_hashes(mut self, capture: bool) -> Self {
        self.capture_frame_hashes = capture;
        self
    }

    /// Attach a sorted input script to replay during evaluation.
    pub fn with_input_script(mut self, script: InputScript) -> Self {
        self.input_script = Some(script);
        self
    }
}

// ── RunResult ─────────────────────────────────────────────────────────────────

/// The full result of evaluating one ROM.
#[derive(Debug, Clone)]
pub struct RunResult {
    /// High-level execution summary (termination reason, draw count, etc.).
    pub summary: ExecutionSummary,
    /// Flat framebuffer at the end of the run.
    ///
    /// Layout: `[plane0…, plane1…]`, length = `2 × 128 × 64` = 16 384 bytes.
    /// Each element is 0 or 1 (pixel on/off).  Always populated.
    pub framebuffer_flat: Vec<u8>,
}

// ── Core evaluation ───────────────────────────────────────────────────────────

/// Derive the effective per-ROM engine seed from a caller-owned base seed.
pub fn effective_seed(rom: &[u8], base_seed: u64) -> u64 {
    base_seed ^ fnv1a_64(rom)
}

/// Evaluate a single ROM with the provided config.
///
/// Returns `Err` if the ROM is too large to load.
pub fn evaluate(rom: &[u8], config: &EvalConfig) -> Result<RunResult, String> {
    if config.capture_frame_hashes {
        return Err(
            "per-frame hash capture is available only through the CUDA batch evaluator".into(),
        );
    }
    // Derive a per-ROM seed so that different ROMs evaluated in the same
    // batch (potentially in parallel) never share RNG state, even when the
    // caller supplies a fixed base seed.
    let seed = effective_seed(rom, config.seed);
    let mut engine = Engine::new(rom, config.quirks, seed)?;
    let cycles_per_frame = config.cycles_per_frame.max(1);
    engine.set_cycles_per_frame(cycles_per_frame);

    let summary = if let Some(script) = &config.input_script {
        let max_frames = match &config.policy {
            RunPolicy::Frames(n) | RunPolicy::UntilHalt { max_frames: n } => *n,
            RunPolicy::Cycles(n) => n / u64::from(cycles_per_frame),
            RunPolicy::UntilStagnant { max_frames, .. } => *max_frames,
        };
        engine.run_with_input_script(script, max_frames)
    } else {
        engine.run_with_policy(&config.policy)
    };
    Ok(RunResult {
        summary,
        framebuffer_flat: engine.framebuffer_flat(),
    })
}

/// Evaluate multiple ROMs sequentially, one per element.
pub fn evaluate_batch(roms: &[&[u8]], config: &EvalConfig) -> Vec<Result<RunResult, String>> {
    roms.iter().map(|rom| evaluate(rom, config)).collect()
}

/// Evaluate multiple ROMs in parallel using rayon.
#[cfg(feature = "parallel")]
pub fn evaluate_batch_parallel(
    roms: &[&[u8]],
    config: &EvalConfig,
) -> Vec<Result<RunResult, String>> {
    use rayon::prelude::*;
    roms.par_iter().map(|rom| evaluate(rom, config)).collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal ROM: jump to itself (infinite loop).
    fn loop_rom() -> Vec<u8> {
        vec![0x12, 0x00] // JP 0x200
    }

    /// ROM that draws then loops: CLEAR, set V0=5, V1=5, load I with font,
    /// draw 5-byte sprite at (V0,V1), jump to self.
    fn draw_rom() -> Vec<u8> {
        vec![
            0x00, 0xE0, // CLS
            0x60, 0x05, // LD V0, 5
            0x61, 0x05, // LD V1, 5
            0xF0, 0x29, // LD F, V0  (I = address of digit-5 sprite)
            0xD0, 0x15, // DRW V0, V1, 5
            0x12, 0x0A, // JP $20A  (jump to DRW)
        ]
    }

    #[test]
    fn evaluate_loop_rom() {
        let rom = loop_rom();
        let config = EvalConfig::new(RunPolicy::Frames(10));
        let result = evaluate(&rom, &config).unwrap();
        assert_eq!(result.summary.frames, 10);
        assert!(result.summary.boot_success);
        assert_eq!(result.summary.draw_count, 0);
    }

    #[test]
    fn cpu_evaluation_rejects_cuda_only_frame_hash_capture() {
        let rom = loop_rom();
        let config = EvalConfig::new(RunPolicy::Frames(5)).with_frame_hashes(true);
        assert!(
            evaluate(&rom, &config)
                .unwrap_err()
                .contains("only through the CUDA batch evaluator")
        );
    }

    #[test]
    fn evaluate_batch_returns_all() {
        let a = loop_rom();
        let b = draw_rom();
        let roms: Vec<&[u8]> = vec![&a, &b];
        let config = EvalConfig::new(RunPolicy::Frames(5));
        let results = evaluate_batch(&roms, &config);
        assert_eq!(results.len(), 2);
        assert!(results[0].is_ok());
        assert!(results[1].is_ok());
    }

    #[test]
    fn scripted_cycle_policy_uses_the_engine_effective_frame_rate() {
        let rom = loop_rom();
        let config = EvalConfig::new(RunPolicy::Cycles(3))
            .with_cycles_per_frame(0)
            .with_input_script(Vec::new());
        let result = evaluate(&rom, &config).expect("zero cycles-per-frame is clamped");

        assert_eq!(result.summary.cycles, 3);
        assert_eq!(result.summary.frames, 3);
        assert_eq!(
            result.summary.termination,
            crate::summary::TerminationReason::Timeout
        );
    }
}
