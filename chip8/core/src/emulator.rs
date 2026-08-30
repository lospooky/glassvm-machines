use std::collections::HashSet;

use crate::configuration::QuirksConfig;
use crate::event::{Event, EventLog};
use crate::input::{InputScript, KeyAction};
use crate::machine::coverage::CoverageTracker;
use crate::machine::cpu::{Cpu, StepResult};
use crate::snapshot::Snapshot;
use crate::summary::{ExecutionSummary, TerminationReason};
use crate::trace::{CompactState, FrameTimerTraceRecord, StepTraceRecord, TimerState};

/// The result of stepping a single 60 Hz frame.
#[derive(Debug, Clone)]
pub enum FrameResult {
    /// Frame completed normally.
    Ok,
    /// CPU executed a halt instruction (`00FD`).
    Halted,
    /// A non-recoverable error occurred.
    Error(StepResult),
}

/// High-level deterministic execution engine wrapping [`Cpu`].
///
/// `Engine` adds on top of the raw CPU:
/// - Event collection ([`EventLog`])
/// - Frame stepping with timer management
/// - Input scripting
/// - Snapshot save / load
/// - Execution summary generation
///
/// This is the primary API for programmatic ROM execution. The `chip8_desktop`
/// binary wraps this for interactive use.
pub struct Engine {
    pub cpu: Cpu,
    events: EventLog,
    coverage: CoverageTracker,
    cycle_count: u64,
    frame_count: u64,
    cycles_per_frame: u32,
    rom_hash: u64,
    /// The RNG seed passed at construction — stored so a Replay can reproduce the run.
    pub initial_seed: u64,
    unique_frame_hashes: HashSet<u64>,
    /// Frame hash recorded after every `step_frame()` call — used by metrics.
    frame_hash_history: Vec<u64>,
    /// Incremental bounded frame statistics used by interestingness metrics.
    frame_metrics: crate::metrics::FrameMetricsAccumulator,
    /// Frame indices (0-based) that had at least one key-press or key-release event.
    input_frame_indices: Vec<u64>,
    /// Keypad mask at end of the previous frame — used to detect input changes.
    last_key_mask: u16,
    /// Exact execution-step records requested by an external observer.
    /// Disabled by default to preserve the normal evaluation hot path.
    record_steps: bool,
    step_records: Vec<StepTraceRecord>,
    frame_timer_records: Vec<FrameTimerTraceRecord>,
}

impl Engine {
    /// Create a new engine, loading `rom` into memory at `ROM_START`.
    ///
    /// Returns `Err` if the ROM is empty or too large. The `seed` is used
    /// to initialise the per-instance RNG (used by `CXNN`).
    pub fn new(rom: &[u8], quirks: QuirksConfig, seed: u64) -> Result<Self, String> {
        let rom_hash = fnv1a_64(rom);
        let mut cpu = Cpu::new(quirks);
        cpu.rng = crate::machine::randomness::Rng::new(seed);
        cpu.load_rom(rom)?;
        Ok(Self {
            cpu,
            events: EventLog::new(),
            coverage: CoverageTracker::new(),
            cycle_count: 0,
            frame_count: 0,
            cycles_per_frame: 12,
            rom_hash,
            initial_seed: seed,
            unique_frame_hashes: HashSet::new(),
            frame_hash_history: Vec::new(),
            frame_metrics: crate::metrics::FrameMetricsAccumulator::default(),
            input_frame_indices: Vec::new(),
            last_key_mask: 0,
            record_steps: false,
            step_records: Vec::new(),
            frame_timer_records: Vec::new(),
        })
    }

    /// Override the default instructions-per-frame (default: 12).
    pub fn with_cycles_per_frame(mut self, n: u32) -> Self {
        self.cycles_per_frame = n;
        self
    }

    // ── Stepping ──────────────────────────────────────────────────────────────

    /// Execute one instruction and return its result.
    pub fn step(&mut self) -> StepResult {
        self.step_with_frame(None)
    }

    /// Execute one instruction attributed to the current 60 Hz frame.
    ///
    /// This is the streaming-observer counterpart to [`Self::step_frame`].
    /// After exactly [`Self::cycles_per_frame`] successful calls, the caller
    /// must call [`Self::finish_frame`] to tick timers and record frame
    /// metrics. A halt or error leaves the frame unfinished, matching
    /// [`Self::step_frame`].
    pub fn step_in_frame(&mut self) -> StepResult {
        self.step_with_frame(Some(self.frame_count))
    }

    #[inline]
    fn step_with_frame(&mut self, frame: Option<u64>) -> StepResult {
        if self.record_steps {
            self.step_recorded(frame)
        } else {
            self.step_unrecorded()
        }
    }

    /// Existing evaluation hot path. Detailed recording dispatches elsewhere
    /// before any trace-only state inspection or allocation occurs.
    #[inline]
    fn step_unrecorded(&mut self) -> StepResult {
        let pc_before = self.cpu.pc;
        let opcode = self.cpu.fetch_opcode();
        let prev_len = self.events.len();
        self.execute_step(pc_before, opcode, prev_len)
    }

    fn step_recorded(&mut self, frame: Option<u64>) -> StepResult {
        let step = self.cycle_count;
        let pc_before = self.cpu.pc;
        let instruction_executed =
            !self.cpu.halted && self.cpu.key_wait == crate::cpu::KeyWait::None;
        let opcode = self.cpu.fetch_opcode();
        let trace_opcode = instruction_executed.then_some(opcode);
        let before = CompactState::capture(&self.cpu);
        let prev_len = self.events.len();

        self.events.begin_memory_capture();
        let result = self.execute_step(pc_before, opcode, prev_len);
        let (memory_reads, memory_changes) = self.events.finish_memory_capture();
        let events = self.events.as_slice()[prev_len..].to_vec();

        self.step_records.push(StepTraceRecord {
            step,
            frame,
            pc: pc_before,
            opcode: trace_opcode,
            result: result.clone(),
            before,
            after: CompactState::capture(&self.cpu),
            events,
            memory_reads,
            memory_changes,
        });
        result
    }

    #[inline]
    fn execute_step(&mut self, pc_before: u16, opcode: u16, prev_len: usize) -> StepResult {
        let result = self.cpu.tick(&mut self.events);
        let pc_after = self.cpu.pc;
        self.coverage.record_step(pc_before, pc_after, opcode);
        self.coverage.record_stack_depth(self.cpu.sp);
        // Scan only the events emitted by this tick.
        for ev in &self.events.as_slice()[prev_len..] {
            if let Event::MemoryWrite { addr, len } = ev {
                self.coverage.record_memory_write(*addr, *len);
            }
        }
        self.cycle_count += 1;
        result
    }

    /// Execute one 60 Hz frame (`cycles_per_frame` instructions + one
    /// `tick_timers()` call). Hashes the resulting framebuffer for
    /// unique-frame tracking.
    pub fn step_frame(&mut self) -> FrameResult {
        for _ in 0..self.cycles_per_frame {
            match self.step_in_frame() {
                StepResult::Ok | StepResult::WaitingForKey => {}
                StepResult::Halted => return FrameResult::Halted,
                other => return FrameResult::Error(other),
            }
        }
        self.finish_frame();
        FrameResult::Ok
    }

    /// Complete the current frame after its instructions have been stepped.
    ///
    /// Machine-bundle observers use this split boundary to forward each exact
    /// instruction record before executing the next instruction, while
    /// retaining the same timer, input, framebuffer, and metric semantics as
    /// [`Self::step_frame`].
    pub fn finish_frame(&mut self) {
        let frame = self.frame_count;
        if self.record_steps {
            let before = TimerState::capture(&self.cpu);
            self.cpu.tick_timers();
            let after = TimerState::capture(&self.cpu);
            if before != after {
                self.frame_timer_records.push(FrameTimerTraceRecord {
                    step: self.cycle_count,
                    frame,
                    before,
                    after,
                });
            }
        } else {
            self.cpu.tick_timers();
        }
        let fh = self
            .frame_metrics
            .record_frame(&self.cpu.display.buf, self.cpu.display.hires);
        self.unique_frame_hashes.insert(fh);
        self.frame_hash_history.push(fh);
        // Detect frames where key state changed (input was injected by caller
        // before this step_frame call).
        let cur_mask = self.cpu.keys.as_mask();
        if cur_mask != self.last_key_mask {
            self.input_frame_indices.push(self.frame_count);
            self.last_key_mask = cur_mask;
        }
        self.frame_count += 1;
    }

    /// Run for up to `max_cycles` instructions and return an
    /// [`ExecutionSummary`].
    pub fn run_cycles(&mut self, max_cycles: u64) -> ExecutionSummary {
        for _ in 0..max_cycles {
            match self.step() {
                StepResult::Ok | StepResult::WaitingForKey => {}
                StepResult::Halted => {
                    return self.build_summary(TerminationReason::Completed);
                }
                StepResult::InvalidOpcode(_) => {
                    return self.build_summary(TerminationReason::InvalidOpcode);
                }
                StepResult::StackOverflow => {
                    return self.build_summary(TerminationReason::StackOverflow);
                }
                StepResult::StackUnderflow => {
                    return self.build_summary(TerminationReason::StackUnderflow);
                }
                StepResult::MemoryFault(_) => {
                    return self.build_summary(TerminationReason::MemoryFault);
                }
            }
        }
        self.build_summary(TerminationReason::Timeout)
    }

    /// Run for up to `max_frames` 60 Hz frames and return an
    /// [`ExecutionSummary`].
    pub fn run_frames(&mut self, max_frames: u64) -> ExecutionSummary {
        for _ in 0..max_frames {
            match self.step_frame() {
                FrameResult::Ok => {}
                FrameResult::Halted => {
                    return self.build_summary(TerminationReason::Completed);
                }
                FrameResult::Error(StepResult::InvalidOpcode(_)) => {
                    return self.build_summary(TerminationReason::InvalidOpcode);
                }
                FrameResult::Error(StepResult::StackOverflow) => {
                    return self.build_summary(TerminationReason::StackOverflow);
                }
                FrameResult::Error(StepResult::StackUnderflow) => {
                    return self.build_summary(TerminationReason::StackUnderflow);
                }
                FrameResult::Error(StepResult::MemoryFault(_)) => {
                    return self.build_summary(TerminationReason::MemoryFault);
                }
                FrameResult::Error(_) => {
                    return self.build_summary(TerminationReason::InvalidOpcode);
                }
            }
        }
        self.build_summary(TerminationReason::Timeout)
    }

    // ── Input ─────────────────────────────────────────────────────────────────

    pub fn press_key(&mut self, key: u8) {
        self.cpu.keys.press(key);
    }

    pub fn release_key(&mut self, key: u8) {
        self.cpu.keys.release(key);
    }

    pub fn set_key_mask(&mut self, mask: u16) {
        self.cpu.keys.from_mask(mask);
    }

    /// Run with a scripted input sequence for up to `max_frames` frames.
    ///
    /// The `script` must be sorted by ascending `frame` index. Key events
    /// are applied before each frame is executed.
    pub fn run_with_input_script(
        &mut self,
        script: &InputScript,
        max_frames: u64,
    ) -> ExecutionSummary {
        let mut idx = 0usize;

        for frame in 0..max_frames {
            while idx < script.len() && script[idx].frame == frame {
                let ev = &script[idx];
                match ev.action {
                    KeyAction::Press => self.press_key(ev.key),
                    KeyAction::Release => self.release_key(ev.key),
                }
                idx += 1;
            }
            match self.step_frame() {
                FrameResult::Ok => {}
                FrameResult::Halted => {
                    return self.build_summary(TerminationReason::Completed);
                }
                FrameResult::Error(StepResult::InvalidOpcode(_)) => {
                    return self.build_summary(TerminationReason::InvalidOpcode);
                }
                FrameResult::Error(StepResult::StackOverflow) => {
                    return self.build_summary(TerminationReason::StackOverflow);
                }
                FrameResult::Error(StepResult::StackUnderflow) => {
                    return self.build_summary(TerminationReason::StackUnderflow);
                }
                FrameResult::Error(StepResult::MemoryFault(_)) => {
                    return self.build_summary(TerminationReason::MemoryFault);
                }
                FrameResult::Error(_) => {
                    return self.build_summary(TerminationReason::InvalidOpcode);
                }
            }
        }

        if self.cpu.key_wait != crate::cpu::KeyWait::None {
            self.build_summary(TerminationReason::WaitingForInput)
        } else {
            self.build_summary(TerminationReason::Timeout)
        }
    }

    /// Unified entry-point: run the engine according to `policy` and return
    /// an [`ExecutionSummary`].
    pub fn run_with_policy(&mut self, policy: &crate::policy::RunPolicy) -> ExecutionSummary {
        use crate::policy::RunPolicy;
        match policy {
            RunPolicy::Frames(n) => self.run_frames(*n),
            RunPolicy::Cycles(n) => self.run_cycles(*n),
            RunPolicy::UntilHalt { max_frames } => {
                for _ in 0..*max_frames {
                    match self.step_frame() {
                        FrameResult::Ok => {}
                        FrameResult::Halted => {
                            return self.build_summary(TerminationReason::Completed);
                        }
                        FrameResult::Error(StepResult::InvalidOpcode(_)) => {
                            return self.build_summary(TerminationReason::InvalidOpcode);
                        }
                        FrameResult::Error(StepResult::StackOverflow) => {
                            return self.build_summary(TerminationReason::StackOverflow);
                        }
                        FrameResult::Error(StepResult::StackUnderflow) => {
                            return self.build_summary(TerminationReason::StackUnderflow);
                        }
                        FrameResult::Error(StepResult::MemoryFault(_)) => {
                            return self.build_summary(TerminationReason::MemoryFault);
                        }
                        FrameResult::Error(_) => {
                            return self.build_summary(TerminationReason::InvalidOpcode);
                        }
                    }
                }
                self.build_summary(TerminationReason::Timeout)
            }
            RunPolicy::UntilStagnant {
                max_frames,
                window,
                threshold,
            } => {
                // Ring-buffer of recent frame hashes for stagnation detection.
                let win = (*window).max(1) as usize;
                let mut recent: Vec<u64> = Vec::with_capacity(win);

                for _ in 0..*max_frames {
                    match self.step_frame() {
                        FrameResult::Ok => {}
                        FrameResult::Halted => {
                            return self.build_summary(TerminationReason::Completed);
                        }
                        FrameResult::Error(StepResult::InvalidOpcode(_)) => {
                            return self.build_summary(TerminationReason::InvalidOpcode);
                        }
                        FrameResult::Error(StepResult::StackOverflow) => {
                            return self.build_summary(TerminationReason::StackOverflow);
                        }
                        FrameResult::Error(StepResult::StackUnderflow) => {
                            return self.build_summary(TerminationReason::StackUnderflow);
                        }
                        FrameResult::Error(StepResult::MemoryFault(_)) => {
                            return self.build_summary(TerminationReason::MemoryFault);
                        }
                        FrameResult::Error(_) => {
                            return self.build_summary(TerminationReason::InvalidOpcode);
                        }
                    }

                    let fh = *self
                        .frame_hash_history
                        .last()
                        .expect("successful step_frame records a frame hash");
                    if recent.len() == win {
                        recent.remove(0);
                    }
                    recent.push(fh);

                    if recent.len() == win {
                        let unique = recent
                            .iter()
                            .collect::<std::collections::HashSet<_>>()
                            .len();
                        let ratio = unique as f32 / win as f32;
                        if ratio < *threshold {
                            return self.build_summary(TerminationReason::Timeout);
                        }
                    }
                }
                self.build_summary(TerminationReason::Timeout)
            }
        }
    }

    // ── Snapshots ─────────────────────────────────────────────────────────────

    pub fn save_snapshot(&self) -> Snapshot {
        self.cpu.save_snapshot()
    }

    /// Restore CPU state from a snapshot. Engine-level counters (cycle count,
    /// frame count, unique frame hashes, event log) are reset, since the
    /// snapshot only captures raw CPU state.
    pub fn load_snapshot(&mut self, snap: &Snapshot) {
        self.cpu.load_snapshot(snap);
        self.events.clear();
        self.unique_frame_hashes.clear();
        self.frame_hash_history.clear();
        self.frame_metrics = crate::metrics::FrameMetricsAccumulator::default();
        self.input_frame_indices.clear();
        self.last_key_mask = self.cpu.keys.as_mask();
        self.coverage.reset();
        self.cycle_count = 0;
        self.frame_count = 0;
        self.step_records.clear();
        self.frame_timer_records.clear();
    }

    // ── Framebuffer ───────────────────────────────────────────────────────────

    /// Raw display buffer: two bitplanes × (128 × 64) pixels, one byte each.
    pub fn get_framebuffer(&self) -> &[[u8; crate::display::BUF_SIZE]; 2] {
        &self.cpu.display.buf
    }

    /// Flat framebuffer as a contiguous `Vec<u8>` suitable for numpy interop.
    ///
    /// Layout: `[plane0_pixel0, plane0_pixel1, …, plane1_pixel0, …]`
    /// Length: `2 × BUF_SIZE` = `2 × 128 × 64` = 16 384 bytes.
    /// Each element is 0 or 1.  In lores mode each logical 64×32 pixel is
    /// stored as a 2×2 block in the 128×64 backing buffer (matching
    /// [`Display`]'s internal layout).
    pub fn framebuffer_flat(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 * crate::display::BUF_SIZE);
        out.extend_from_slice(&self.cpu.display.buf[0]);
        out.extend_from_slice(&self.cpu.display.buf[1]);
        out
    }

    /// FNV-1a 64-bit hash of the current framebuffer (both planes combined).
    pub fn frame_hash(&self) -> u64 {
        // Hash both planes together in one pass, no extra allocation.
        let mut h: u64 = 14695981039346656037;
        for &b in self.cpu.display.buf[0]
            .iter()
            .chain(self.cpu.display.buf[1].iter())
        {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        h
    }

    /// FNV-1a 64-bit hash of the full CPU-visible state (registers, memory,
    /// display, timers).  Used for state-fingerprinting deduplication.
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 14695981039346656037;
        let mix = |h: &mut u64, b: u8| {
            *h ^= b as u64;
            *h = h.wrapping_mul(1099511628211);
        };
        for &b in &self.cpu.mem {
            mix(&mut h, b);
        }
        for &b in &self.cpu.v {
            mix(&mut h, b);
        }
        for b in self.cpu.i.to_le_bytes() {
            mix(&mut h, b);
        }
        for b in self.cpu.pc.to_le_bytes() {
            mix(&mut h, b);
        }
        mix(&mut h, self.cpu.sp);
        mix(&mut h, self.cpu.dt);
        mix(&mut h, self.cpu.st);
        h
    }

    /// Count pixels that differ between two raw framebuffer slices.
    pub fn frame_diff(a: &[u8], b: &[u8]) -> u32 {
        assert_eq!(a.len(), b.len());
        a.iter().zip(b.iter()).filter(|(a, b)| a != b).count() as u32
    }

    // ── Coverage ──────────────────────────────────────────────────────────────

    /// A snapshot of coverage recorded so far in this run.
    pub fn coverage(&self) -> crate::coverage::CoverageSummary {
        self.coverage.summary()
    }

    // ── Events ────────────────────────────────────────────────────────────────

    /// Drain all events accumulated since the last drain.
    pub fn drain_events(&mut self) -> Vec<Event> {
        self.events.drain()
    }

    /// Inspect events without consuming them.
    pub fn events(&self) -> &[Event] {
        self.events.as_slice()
    }

    /// Enable or disable exact execution-step and frame-timer records.
    ///
    /// Changing the mode clears existing records so observers cannot
    /// accidentally combine traces captured at different detail levels.
    pub fn set_step_recording(&mut self, enabled: bool) {
        self.record_steps = enabled;
        self.step_records.clear();
        self.frame_timer_records.clear();
    }

    pub fn step_recording(&self) -> bool {
        self.record_steps
    }

    pub fn step_records(&self) -> &[StepTraceRecord] {
        &self.step_records
    }

    /// Drain recorded execution steps while keeping detailed recording enabled.
    pub fn drain_step_records(&mut self) -> Vec<StepTraceRecord> {
        std::mem::take(&mut self.step_records)
    }

    pub fn frame_timer_records(&self) -> &[FrameTimerTraceRecord] {
        &self.frame_timer_records
    }

    /// Drain frame-boundary timer transitions while keeping recording enabled.
    pub fn drain_frame_timer_records(&mut self) -> Vec<FrameTimerTraceRecord> {
        std::mem::take(&mut self.frame_timer_records)
    }

    /// Per-frame framebuffer hashes recorded during the run (one per
    /// `step_frame()` call).
    pub fn frame_hash_history(&self) -> &[u64] {
        &self.frame_hash_history
    }

    /// Exact SHA-256 identity of all completed physical composite frames.
    ///
    /// This is always accumulated at the frame-metrics boundary and does not
    /// require retaining per-frame records or framebuffer copies.
    pub fn trajectory_identity(&self) -> crate::metrics::TrajectoryIdentity {
        let identity = self.frame_metrics.trajectory_identity();
        debug_assert_eq!(identity.frame_count, self.frame_count);
        identity
    }

    /// Frame indices (0-based) where at least one key event occurred.
    pub fn input_frame_indices(&self) -> &[u64] {
        &self.input_frame_indices
    }

    /// Compute interestingness metrics over the completed run.
    pub fn compute_interestingness(&self) -> crate::metrics::InterestingnessSummary {
        use crate::metrics;
        let cov = self.coverage.summary();
        let event_counts = metrics::count_events(self.events.as_slice());
        let frame_metrics = self.frame_metrics.aggregate();
        let (loop_period_frames, loop_periodicity) = metrics::frame_loop_periodicity(
            &self.frame_hash_history,
            frame_metrics.mean_frame_delta,
        );
        metrics::InterestingnessSummary {
            frame_entropy: metrics::frame_entropy(&self.frame_hash_history),
            change_rate: metrics::change_rate(&self.frame_hash_history),
            last_change_frame: metrics::last_change_frame(&self.frame_hash_history),
            late_change_rate: metrics::late_change_rate(&self.frame_hash_history),
            late_frame_discovery_rate: metrics::late_frame_discovery_rate(&self.frame_hash_history),
            opcode_diversity: metrics::opcode_diversity(&cov.opcode_counts),
            coverage_growth: metrics::coverage_growth(cov.unique_pcs, self.cycle_count),
            input_responsiveness: metrics::input_responsiveness(
                &self.input_frame_indices,
                &self.frame_hash_history,
            ),
            draw_density: metrics::draw_density(event_counts.draw_count, self.cycle_count),
            mean_lit_fraction: frame_metrics.mean_lit_fraction,
            peak_lit_fraction: frame_metrics.peak_lit_fraction,
            mean_frame_delta: frame_metrics.mean_frame_delta,
            mean_capped_changed_pixels: frame_metrics.mean_capped_changed_pixels,
            broad_transition_share: frame_metrics.broad_transition_share,
            motion_spread: frame_metrics.motion_spread,
            motion_spatial_spread: frame_metrics.motion_spatial_spread,
            motion_axis_balance: frame_metrics.motion_axis_balance,
            frame_delta_cv: frame_metrics.frame_delta_cv,
            mean_edge_density: frame_metrics.mean_edge_density,
            ordered_spatial_structure: frame_metrics.ordered_spatial_structure,
            object_scale_composition: frame_metrics.object_scale_composition,
            repetitive_texture: frame_metrics.repetitive_texture,
            spatial_repeat_autocorrelation: frame_metrics.spatial_repeat_autocorrelation,
            persistent_component_structure: frame_metrics.persistent_component_structure,
            connected_negative_space: frame_metrics.connected_negative_space,
            active_region_fraction: frame_metrics.active_region_fraction,
            mean_change_region_fraction: frame_metrics.mean_change_region_fraction,
            loop_period_frames,
            loop_periodicity,
            clear_count: event_counts.clear_count,
            delay_timer_set_count: event_counts.delay_timer_set_count,
            delay_timer_nonzero_count: event_counts.delay_timer_nonzero_count,
            sound_timer_set_count: event_counts.sound_timer_set_count,
            sound_timer_nonzero_count: event_counts.sound_timer_nonzero_count,
            scroll_count: event_counts.scroll_count,
            composition_stable_foreground: frame_metrics.composition_stable_foreground,
            composition_active_fraction: frame_metrics.composition_active_fraction,
            coherent_change_topology: frame_metrics.coherent_change_topology,
            temporal_overlap_reversal: frame_metrics.temporal_overlap_reversal,
        }
    }

    // ── Summary ───────────────────────────────────────────────────────────────

    pub fn build_summary(&self, termination: TerminationReason) -> ExecutionSummary {
        let event_counts = crate::metrics::count_events(self.events.as_slice());

        let boot_success = self.cycle_count > 0 && termination != TerminationReason::InvalidOpcode;

        let stagnation_score = if self.frame_count == 0 {
            1.0
        } else {
            1.0 - (self.unique_frame_hashes.len() as f32 / self.frame_count as f32)
        };

        ExecutionSummary {
            rom_hash: self.rom_hash,
            cycles: self.cycle_count,
            frames: self.frame_count,
            boot_success,
            termination,
            draw_count: event_counts.draw_count,
            collision_count: event_counts.collision_count,
            input_opcode_count: event_counts.input_opcode_count,
            unique_frame_count: self.unique_frame_hashes.len() as u32,
            coverage: self.coverage.summary(),
            stagnation_score,
            interestingness_score: 0.0,
        }
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    pub fn cycle_count(&self) -> u64 {
        self.cycle_count
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn rom_hash(&self) -> u64 {
        self.rom_hash
    }

    pub fn cycles_per_frame(&self) -> u32 {
        self.cycles_per_frame
    }

    pub fn set_cycles_per_frame(&mut self, n: u32) {
        self.cycles_per_frame = n.max(1);
    }

    pub fn quirks(&self) -> &crate::configuration::QuirksConfig {
        &self.cpu.quirks
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// FNV-1a 64-bit hash — no external dependency required.
pub(crate) fn fnv1a_64(data: &[u8]) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

// suppress unused import lint from Timer which is used in match pattern above
#[allow(unused_imports)]
use crate::event::Timer as _;
