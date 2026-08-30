use crate::configuration::QuirksConfig;
/// Batch evaluation API for running one or many ROMs with a unified config.
///
/// The entry-points are [`evaluate`] (single ROM) and [`evaluate_batch`]
/// (multiple ROMs, optionally parallel with the `parallel` feature).
use crate::emulator::{Engine, fnv1a_64};
use crate::event::Event;
use crate::input::InputScript;
use crate::metrics::{InterestingnessSummary, TrajectoryIdentity};
use crate::policy::RunPolicy;
use crate::summary::ExecutionSummary;

// ── EvalConfig ────────────────────────────────────────────────────────────────

/// Full configuration for a single evaluation run.
///
/// Wraps a [`RunPolicy`] (the stopping criterion) with orthogonal concerns
/// like the RNG seed, quirks mode, and optional recording options.
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
    /// Record a [`FrameRecord`] for every frame?  Adds memory overhead.
    pub record_frames: bool,
    /// Return the full event log in [`RunResult::events`]?
    pub record_events: bool,
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
            record_frames: false,
            record_events: false,
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

    pub fn with_frames(mut self, record: bool) -> Self {
        self.record_frames = record;
        self
    }

    pub fn with_events(mut self, record: bool) -> Self {
        self.record_events = record;
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
    /// Interestingness metrics computed from the run.
    pub interestingness: InterestingnessSummary,
    /// All events emitted during the run, if `EvalConfig::record_events` was set.
    pub events: Vec<Event>,
    /// Per-frame records, if `EvalConfig::record_frames` was set.
    pub frames: Option<Vec<FrameRecord>>,
    /// Exact identity of all completed physical composite frames.
    pub trajectory_identity: TrajectoryIdentity,
    /// Flat framebuffer at the end of the run.
    ///
    /// Layout: `[plane0…, plane1…]`, length = `2 × 128 × 64` = 16 384 bytes.
    /// Each element is 0 or 1 (pixel on/off).  Always populated.
    pub framebuffer_flat: Vec<u8>,
}

/// A single recorded frame.
#[derive(Debug, Clone)]
pub struct FrameRecord {
    /// 0-based frame index.
    pub frame_number: u64,
    /// FNV-1a hash of the framebuffer after this frame.
    pub hash: u64,
    /// Whether the framebuffer changed from the previous frame.
    pub changed: bool,
}

// ── Core evaluation ───────────────────────────────────────────────────────────

/// Derive the effective per-ROM engine seed from a caller-owned base seed.
///
/// Keeping this operation in the core gives evaluators, replayers, and audit
/// tools one authoritative implementation of the seed contract.
pub fn effective_seed(rom: &[u8], base_seed: u64) -> u64 {
    base_seed ^ fnv1a_64(rom)
}

/// Evaluate a single ROM with the provided config.
///
/// Returns `Err` if the ROM is too large to load.
pub fn evaluate(rom: &[u8], config: &EvalConfig) -> Result<RunResult, String> {
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
    let interestingness = engine.compute_interestingness();

    let events = if config.record_events {
        engine.drain_events()
    } else {
        Vec::new()
    };

    let frames = if config.record_frames {
        let hashes = engine.frame_hash_history();
        let records: Vec<FrameRecord> = hashes
            .iter()
            .enumerate()
            .map(|(i, &hash)| FrameRecord {
                frame_number: i as u64,
                hash,
                changed: i == 0 || hashes[i] != hashes[i - 1],
            })
            .collect();
        Some(records)
    } else {
        None
    };

    Ok(RunResult {
        summary,
        interestingness,
        events,
        frames,
        trajectory_identity: engine.trajectory_identity(),
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
        assert!(result.frames.is_none());
    }

    #[test]
    fn evaluate_with_frame_recording() {
        let rom = loop_rom();
        let config = EvalConfig::new(RunPolicy::Frames(5)).with_frames(true);
        let result = evaluate(&rom, &config).unwrap();
        assert_eq!(result.trajectory_identity.frame_count, 5);
        let frames = result.frames.unwrap();
        assert_eq!(frames.len(), 5);
    }

    #[test]
    fn trajectory_identity_is_independent_of_frame_recording() {
        let rom = draw_rom();
        let without_frames = evaluate(&rom, &EvalConfig::new(RunPolicy::Frames(7))).unwrap();
        let with_frames = evaluate(
            &rom,
            &EvalConfig::new(RunPolicy::Frames(7)).with_frames(true),
        )
        .unwrap();

        assert_eq!(
            without_frames.trajectory_identity,
            with_frames.trajectory_identity
        );
        assert!(without_frames.frames.is_none());
        assert_eq!(with_frames.frames.unwrap().len(), 7);
    }

    #[test]
    fn evaluate_draw_rom_nonzero_density() {
        let rom = draw_rom();
        let config = EvalConfig::new(RunPolicy::Frames(30));
        let result = evaluate(&rom, &config).unwrap();
        assert!(result.interestingness.draw_density > 0.0);
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
