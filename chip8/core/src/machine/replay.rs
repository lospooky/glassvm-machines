use crate::configuration::QuirksConfig;
/// Deterministic replay recording and playback.
///
/// A [`Replay`] captures everything needed to reproduce an execution
/// exactly: the ROM content (as a hash for identification, full bytes
/// are external), the RNG seed, quirks, cycles-per-frame, and the full
/// input script.
///
/// [`ReplayRecorder`] is attached to a live engine run to capture key
/// events as they are fed in, then finalised into a [`Replay`] value.
use crate::emulator::Engine;
use crate::input::{InputEvent, InputScript, KeyAction};
use crate::summary::ExecutionSummary;

// ── Replay ────────────────────────────────────────────────────────────────────

/// Everything needed to reproduce a past execution.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Replay {
    /// FNV-1a hash of the ROM bytes (identification only; the actual ROM is
    /// supplied when calling [`Replay::play`]).
    pub rom_hash: u64,
    /// Initial RNG seed used during the original run.
    pub seed: u64,
    /// Quirks configuration in effect during the original run.
    pub quirks: QuirksConfig,
    /// Number of instruction cycles executed per 60 Hz frame.
    pub cycles_per_frame: u32,
    /// Scripted key events captured from the original run.
    pub input_script: InputScript,
    /// Total number of frames the original run lasted.
    pub max_frames: u64,
}

impl Replay {
    /// Re-execute this replay against `rom` and return a fresh
    /// [`ExecutionSummary`].
    pub fn play(&self, rom: &[u8]) -> Result<ExecutionSummary, String> {
        if self.cycles_per_frame == 0 {
            return Err("replay cycles_per_frame must be non-zero".into());
        }
        let mut engine = Engine::new(rom, self.quirks, self.seed)?;
        if engine.rom_hash() != self.rom_hash {
            return Err(format!(
                "replay ROM hash mismatch: expected {:016x}, got {:016x}",
                self.rom_hash,
                engine.rom_hash()
            ));
        }
        engine.set_cycles_per_frame(self.cycles_per_frame);
        Ok(engine.run_with_input_script(&self.input_script, self.max_frames))
    }
}

// ── ReplayRecorder ────────────────────────────────────────────────────────────

/// Attached to an engine run to record the input stream so it can later be
/// reproduced as a [`Replay`].
///
/// Call [`ReplayRecorder::record_frame`] once per frame with the current key
/// bitmask, then call [`ReplayRecorder::finish`] to obtain the final recorded
/// [`InputScript`].
pub struct ReplayRecorder {
    events: Vec<InputEvent>,
    prev_mask: u16,
}

impl ReplayRecorder {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            prev_mask: 0,
        }
    }

    /// Diff `current_mask` against the previous mask and emit press/release
    /// [`InputEvent`]s for all changed keys at `frame`.
    pub fn record_frame(&mut self, frame: u64, current_mask: u16) {
        let changed = self.prev_mask ^ current_mask;
        for key in 0u8..16 {
            let bit = 1u16 << key;
            if changed & bit != 0 {
                let action = if current_mask & bit != 0 {
                    KeyAction::Press
                } else {
                    KeyAction::Release
                };
                self.events.push(InputEvent { frame, key, action });
            }
        }
        self.prev_mask = current_mask;
    }

    /// Consume the recorder and return the accumulated input script.
    pub fn finish(self) -> InputScript {
        self.events
    }
}

impl Default for ReplayRecorder {
    fn default() -> Self {
        Self::new()
    }
}

// ── Engine extension methods ──────────────────────────────────────────────────

impl Engine {
    /// Run for `max_frames` frames with a recorder active, capturing key
    /// events from `key_masks` (one entry per frame), and return a
    /// [`Replay`] alongside the [`ExecutionSummary`].
    ///
    /// `key_masks` may be shorter than `max_frames`; missing frames are
    /// treated as all-keys-released.
    pub fn run_and_record(
        &mut self,
        max_frames: u64,
        key_masks: &[u16],
    ) -> (ExecutionSummary, Replay) {
        use crate::cpu::StepResult;
        use crate::emulator::FrameResult;
        use crate::summary::TerminationReason;

        let mut recorder = ReplayRecorder::new();
        let mut reason = TerminationReason::Timeout;

        'outer: for frame in 0..max_frames {
            let mask = key_masks.get(frame as usize).copied().unwrap_or(0);
            recorder.record_frame(frame, mask);

            // Apply key state from mask.
            for key in 0u8..16 {
                let bit = 1u16 << key;
                if mask & bit != 0 {
                    self.press_key(key);
                } else {
                    self.release_key(key);
                }
            }

            match self.step_frame() {
                FrameResult::Ok => {}
                FrameResult::Halted => {
                    reason = TerminationReason::Completed;
                    break 'outer;
                }
                FrameResult::Error(StepResult::StackOverflow) => {
                    reason = TerminationReason::StackOverflow;
                    break 'outer;
                }
                FrameResult::Error(StepResult::StackUnderflow) => {
                    reason = TerminationReason::StackUnderflow;
                    break 'outer;
                }
                FrameResult::Error(_) => {
                    reason = TerminationReason::InvalidOpcode;
                    break 'outer;
                }
            }
        }

        let summary = self.build_summary(reason);
        let cycles_per_frame = self.cycles_per_frame();
        let replay = Replay {
            rom_hash: self.rom_hash(),
            seed: self.initial_seed,
            quirks: *self.quirks(),
            cycles_per_frame,
            input_script: recorder.finish(),
            max_frames,
        };
        (summary, replay)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_diff_press_release() {
        let mut r = ReplayRecorder::new();
        // Frame 0: press key 0
        r.record_frame(0, 0b0001);
        // Frame 1: no change
        r.record_frame(1, 0b0001);
        // Frame 2: release key 0
        r.record_frame(2, 0b0000);

        let events = r.finish();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].action, KeyAction::Press);
        assert_eq!(events[0].key, 0);
        assert_eq!(events[0].frame, 0);
        assert_eq!(events[1].action, KeyAction::Release);
        assert_eq!(events[1].key, 0);
        assert_eq!(events[1].frame, 2);
    }

    #[test]
    fn recorder_empty_when_no_changes() {
        let mut r = ReplayRecorder::new();
        r.record_frame(0, 0);
        r.record_frame(1, 0);
        r.record_frame(2, 0);
        assert!(r.finish().is_empty());
    }

    #[test]
    fn playback_rejects_the_wrong_rom_before_running() {
        let rom = [0x12, 0x00];
        let mut engine = Engine::new(&rom, QuirksConfig::default(), 7).unwrap();
        let (_, replay) = engine.run_and_record(1, &[0]);

        assert!(replay.play(&rom).is_ok());
        let error = replay
            .play(&[0x12, 0x02])
            .expect_err("same-length ROM with different bytes must fail");
        assert!(error.contains("ROM hash mismatch"));
    }
}
