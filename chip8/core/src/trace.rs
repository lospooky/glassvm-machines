//! Opt-in, exact execution-step records for architecture adapters.
//!
//! The normal emulator path leaves step recording disabled, so it does not
//! clone compact state or allocate per-step records. GlassVM observation
//! plans enable this only when they require instruction/access fidelity.

use crate::event::Event;
use crate::machine::cpu::{Cpu, KeyWait, StepResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactState {
    pub v: [u8; 16],
    pub i: u16,
    pub pc: u16,
    pub stack: [u16; 16],
    pub sp: u8,
    pub dt: u8,
    pub st: u8,
    pub display_hires: bool,
    pub display_plane: u8,
    pub keys: u16,
    pub rng_state: u64,
    pub flags: [u8; 16],
    pub audio_buf: [u8; 16],
    pub audio_pitch: u8,
    pub halted: bool,
    pub key_wait: KeyWait,
}

impl CompactState {
    pub(crate) fn capture(cpu: &Cpu) -> Self {
        Self {
            v: cpu.v,
            i: cpu.i,
            pc: cpu.pc,
            stack: cpu.stack,
            sp: cpu.sp,
            dt: cpu.dt,
            st: cpu.st,
            display_hires: cpu.display.hires,
            display_plane: cpu.display.plane,
            keys: cpu.keys.as_mask(),
            rng_state: cpu.rng.state(),
            flags: cpu.flags,
            audio_buf: cpu.audio_buf,
            audio_pitch: cpu.audio_pitch,
            halted: cpu.halted,
            key_wait: cpu.key_wait,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryChange {
    pub address: u16,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRead {
    pub address: u16,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepTraceRecord {
    /// Zero-based execution-step attempt across this engine session.
    pub step: u64,
    /// Zero-based frame containing this step, or `None` for cycle/direct
    /// stepping outside [`crate::emulator::Engine::step_frame`].
    pub frame: Option<u64>,
    /// PC before the instruction or key-wait transition.
    pub pc: u16,
    /// Instruction fetched at `pc`, or `None` while halted/key-waiting.
    pub opcode: Option<u16>,
    pub result: StepResult,
    pub before: CompactState,
    pub after: CompactState,
    /// Native semantic events emitted by this exact execution step.
    pub events: Vec<Event>,
    /// Exact native memory reads performed during this execution step.
    pub memory_reads: Vec<MemoryRead>,
    /// Exact bytes changed by native memory-write instructions.
    pub memory_changes: Vec<MemoryChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerState {
    pub delay: u8,
    pub sound: u8,
}

impl TimerState {
    pub(crate) fn capture(cpu: &Cpu) -> Self {
        Self {
            delay: cpu.dt,
            sound: cpu.st,
        }
    }
}

/// Timer state transition caused by the 60 Hz frame boundary rather than an
/// instruction. Keeping it separate prevents timer decrements from being
/// falsely attributed to the final instruction in a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameTimerTraceRecord {
    /// Step coordinate immediately after the frame's instruction attempts.
    pub step: u64,
    /// Zero-based frame whose timer tick produced this transition.
    pub frame: u64,
    pub before: TimerState,
    pub after: TimerState,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::QuirksConfig;
    use crate::cpu::{KeyWait, StepResult};
    use crate::emulator::{Engine, FrameResult};
    use crate::event::{Event, Timer};

    fn engine(rom: &[u8]) -> Engine {
        Engine::new(rom, QuirksConfig::default(), 0x1234).expect("valid test ROM")
    }

    #[test]
    fn recording_is_disabled_by_default_and_non_interfering() {
        let rom = [
            0x60, 0x01, // V0 = 1
            0xA3, 0x00, // I = 0x300
            0xF0, 0x55, // [I] = V0
            0xC1, 0xFF, // V1 = random byte
            0xD0, 0x11, // draw one row from 0x300
            0x12, 0x00, // loop
        ];
        let mut ordinary = engine(&rom);
        let mut observed = engine(&rom);
        ordinary.set_cycles_per_frame(6);
        observed.set_cycles_per_frame(6);
        observed.set_step_recording(true);

        assert!(!ordinary.step_recording());
        assert!(matches!(ordinary.step_frame(), FrameResult::Ok));
        assert!(matches!(observed.step_frame(), FrameResult::Ok));

        assert_eq!(
            CompactState::capture(&ordinary.cpu),
            CompactState::capture(&observed.cpu)
        );
        assert_eq!(ordinary.cpu.mem, observed.cpu.mem);
        assert_eq!(ordinary.cpu.display.buf, observed.cpu.display.buf);
        assert_eq!(ordinary.events(), observed.events());
        assert_eq!(ordinary.cycle_count(), observed.cycle_count());
        assert_eq!(ordinary.frame_count(), observed.frame_count());
        assert_eq!(ordinary.frame_hash_history(), observed.frame_hash_history());
        assert!(ordinary.step_records().is_empty());
        assert!(ordinary.frame_timer_records().is_empty());
        assert_eq!(observed.step_records().len(), 6);
        assert!(
            observed
                .step_records()
                .iter()
                .all(|record| record.frame == Some(0))
        );
    }

    #[test]
    fn records_exact_context_native_event_boundaries_and_compact_state() {
        let rom = [
            0x60, 0x07, // 0x200: V0 = 7
            0x22, 0x08, // 0x202: call 0x208
            0x12, 0x04, // 0x204: loop after return
            0x00, 0x00, // 0x206: filler
            0xF0, 0x15, // 0x208: DT = V0
            0x00, 0xEE, // 0x20A: return
        ];
        let mut engine = engine(&rom);
        engine.set_step_recording(true);
        for _ in 0..4 {
            assert_eq!(engine.step(), StepResult::Ok);
        }

        let records = engine.step_records();
        assert_eq!(records.len(), 4);
        assert_eq!(
            records
                .iter()
                .map(|record| (record.step, record.frame, record.pc, record.opcode))
                .collect::<Vec<_>>(),
            vec![
                (0, None, 0x200, Some(0x6007)),
                (1, None, 0x202, Some(0x2208)),
                (2, None, 0x208, Some(0xF015)),
                (3, None, 0x20A, Some(0x00EE)),
            ]
        );

        assert!(records[0].events.is_empty());
        assert_eq!(records[0].before.v[0], 0);
        assert_eq!(records[0].after.v[0], 7);

        assert_eq!(
            records[1].events,
            vec![
                Event::Call {
                    from: 0x202,
                    to: 0x208,
                },
                Event::StackPush,
            ]
        );
        assert_eq!(records[1].before.sp, 0);
        assert_eq!(records[1].after.sp, 1);
        assert_eq!(records[1].before.stack[0], 0);
        assert_eq!(records[1].after.stack[0], 0x204);

        assert_eq!(
            records[2].events,
            vec![Event::TimerSet {
                timer: Timer::Delay,
                value: 7,
            }]
        );
        assert_eq!(records[2].before.dt, 0);
        assert_eq!(records[2].after.dt, 7);

        assert_eq!(
            records[3].events,
            vec![Event::Return { to: 0x204 }, Event::StackPop]
        );
        assert_eq!(records[3].before.sp, 1);
        assert_eq!(records[3].after.sp, 0);
        assert_eq!(records[3].after.pc, 0x204);
    }

    #[test]
    fn captures_exact_before_and_after_for_every_memory_write_instruction() {
        let rom = [
            0xF1, 0x55, // store V0..V1
            0xF0, 0x33, // BCD V0
            0x50, 0x12, // XO-CHIP store V0..V1
        ];
        let mut engine = engine(&rom);
        engine.set_step_recording(true);

        engine.cpu.i = 0x300;
        engine.cpu.v[0] = 0xAA;
        engine.cpu.v[1] = 0xBB;
        engine.cpu.mem[0x300..0x302].copy_from_slice(&[0x11, 0x22]);
        assert_eq!(engine.step(), StepResult::Ok);

        engine.cpu.i = 0x310;
        engine.cpu.v[0] = 254;
        engine.cpu.mem[0x310..0x313].copy_from_slice(&[9, 9, 9]);
        assert_eq!(engine.step(), StepResult::Ok);

        engine.cpu.i = 0x320;
        engine.cpu.v[0] = 0x12;
        engine.cpu.v[1] = 0x34;
        engine.cpu.mem[0x320..0x322].copy_from_slice(&[0xFE, 0xED]);
        assert_eq!(engine.step(), StepResult::Ok);

        let records = engine.step_records();
        assert_eq!(
            records[0].memory_changes,
            vec![MemoryChange {
                address: 0x300,
                before: vec![0x11, 0x22],
                after: vec![0xAA, 0xBB],
            }]
        );
        assert_eq!(
            records[1].memory_changes,
            vec![MemoryChange {
                address: 0x310,
                before: vec![9, 9, 9],
                after: vec![2, 5, 4],
            }]
        );
        assert_eq!(
            records[2].memory_changes,
            vec![MemoryChange {
                address: 0x320,
                before: vec![0xFE, 0xED],
                after: vec![0x12, 0x34],
            }]
        );
        assert_eq!(
            records
                .iter()
                .map(|record| record.events.clone())
                .collect::<Vec<_>>(),
            vec![
                vec![Event::MemoryWrite {
                    addr: 0x300,
                    len: 2,
                }],
                vec![Event::MemoryWrite {
                    addr: 0x310,
                    len: 3,
                }],
                vec![Event::MemoryWrite {
                    addr: 0x320,
                    len: 2,
                }],
            ]
        );
    }

    #[test]
    fn captures_fx65_and_draw_memory_reads() {
        let mut load = engine(&[0xF1, 0x65]);
        load.cpu.i = 0x300;
        load.cpu.mem[0x300..0x302].copy_from_slice(&[0x11, 0x22]);
        load.set_step_recording(true);
        assert_eq!(load.step(), StepResult::Ok);
        assert_eq!(
            load.step_records()[0].memory_reads,
            vec![MemoryRead {
                address: 0x300,
                bytes: vec![0x11, 0x22],
            }]
        );
        assert_eq!(&load.cpu.v[..2], &[0x11, 0x22]);

        let mut draw = engine(&[0xD0, 0x12]);
        draw.cpu.i = 0x300;
        draw.cpu.mem[0x300..0x302].copy_from_slice(&[0xF0, 0x0F]);
        draw.set_step_recording(true);
        assert_eq!(draw.step(), StepResult::Ok);
        assert_eq!(
            draw.step_records()[0].memory_reads,
            vec![MemoryRead {
                address: 0x300,
                bytes: vec![0xF0, 0x0F],
            }]
        );
        assert!(matches!(
            draw.step_records()[0].events.as_slice(),
            [Event::Draw { n: 2, .. }]
        ));
    }

    #[test]
    fn captures_xo_range_long_operand_and_audio_memory_reads() {
        let mut range = engine(&[0x50, 0x13]);
        range.cpu.i = 0x300;
        range.cpu.mem[0x300..0x302].copy_from_slice(&[0x45, 0x67]);
        range.set_step_recording(true);
        assert_eq!(range.step(), StepResult::Ok);
        assert_eq!(
            range.step_records()[0].memory_reads,
            vec![MemoryRead {
                address: 0x300,
                bytes: vec![0x45, 0x67],
            }]
        );

        let mut long_i = engine(&[0xF0, 0x00, 0xAB, 0xCD]);
        long_i.set_step_recording(true);
        assert_eq!(long_i.step(), StepResult::Ok);
        assert_eq!(
            long_i.step_records()[0].memory_reads,
            vec![MemoryRead {
                address: 0x202,
                bytes: vec![0xAB, 0xCD],
            }]
        );
        assert_eq!(long_i.cpu.i, 0xABCD);

        let mut audio = engine(&[0xF0, 0x02]);
        let pattern = core::array::from_fn::<_, 16, _>(|index| index as u8);
        audio.cpu.i = 0x300;
        audio.cpu.mem[0x300..0x310].copy_from_slice(&pattern);
        audio.set_step_recording(true);
        assert_eq!(audio.step(), StepResult::Ok);
        assert_eq!(
            audio.step_records()[0].memory_reads,
            vec![MemoryRead {
                address: 0x300,
                bytes: pattern.to_vec(),
            }]
        );
        assert_eq!(audio.cpu.audio_buf, pattern);
    }

    #[test]
    fn key_wait_transitions_do_not_fabricate_instruction_opcodes() {
        let mut engine = engine(&[
            0xF0, 0x0A, // wait for key into V0
            0x60, 0x01, // next real instruction
        ]);
        engine.set_step_recording(true);

        assert_eq!(engine.step(), StepResult::Ok);
        engine.press_key(5);
        assert_eq!(engine.step(), StepResult::WaitingForKey);
        engine.release_key(5);
        assert_eq!(engine.step(), StepResult::Ok);
        assert_eq!(engine.step(), StepResult::Ok);

        let records = engine.step_records();
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].opcode, Some(0xF00A));
        assert_eq!(records[0].events, vec![Event::KeyWaitEntered]);
        assert_eq!(records[1].opcode, None);
        assert!(records[1].events.is_empty());
        assert_eq!(records[1].after.key_wait, KeyWait::WaitRelease(0, 5));
        assert_eq!(records[2].opcode, None);
        assert_eq!(records[2].events, vec![Event::KeyWaitResolved { key: 5 }]);
        assert_eq!(records[2].before.v[0], 0);
        assert_eq!(records[2].after.v[0], 5);
        assert_eq!(records[3].opcode, Some(0x6001));
        assert_eq!(records[3].after.v[0], 1);
    }

    #[test]
    fn frame_coordinates_timer_deltas_and_draining_are_exact() {
        let mut engine = engine(&[
            0x60, 0x02, // V0 = 2
            0xF0, 0x15, // DT = 2
            0x12, 0x04, // loop
        ]);
        engine.set_cycles_per_frame(2);
        engine.cpu.st = 1;
        engine.set_step_recording(true);

        assert!(matches!(engine.step_frame(), FrameResult::Ok));
        assert_eq!(
            engine
                .step_records()
                .iter()
                .map(|record| record.frame)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(0)]
        );
        assert_eq!(
            engine.frame_timer_records(),
            &[FrameTimerTraceRecord {
                step: 2,
                frame: 0,
                before: TimerState { delay: 2, sound: 1 },
                after: TimerState { delay: 1, sound: 0 },
            }]
        );

        let drained_steps = engine.drain_step_records();
        let drained_timers = engine.drain_frame_timer_records();
        assert_eq!(drained_steps.len(), 2);
        assert_eq!(drained_timers.len(), 1);
        assert!(engine.step_recording());
        assert!(engine.step_records().is_empty());
        assert!(engine.frame_timer_records().is_empty());

        assert!(matches!(engine.step_frame(), FrameResult::Ok));
        assert!(
            engine
                .step_records()
                .iter()
                .all(|record| record.frame == Some(1))
        );
        assert_eq!(
            engine.frame_timer_records(),
            &[FrameTimerTraceRecord {
                step: 4,
                frame: 1,
                before: TimerState { delay: 1, sound: 0 },
                after: TimerState { delay: 0, sound: 0 },
            }]
        );
    }

    #[test]
    fn reset_clears_records_and_preserves_recording_mode() {
        let mut engine = engine(&[0xF0, 0x15, 0x12, 0x00]);
        engine.cpu.v[0] = 2;
        engine.set_cycles_per_frame(1);
        let initial = engine.save_snapshot();
        engine.set_step_recording(true);

        assert!(matches!(engine.step_frame(), FrameResult::Ok));
        assert!(!engine.step_records().is_empty());
        assert!(!engine.frame_timer_records().is_empty());

        engine.load_snapshot(&initial);
        assert!(engine.step_recording());
        assert!(engine.step_records().is_empty());
        assert!(engine.frame_timer_records().is_empty());
        assert_eq!(engine.step(), StepResult::Ok);
        assert_eq!(engine.step_records().len(), 1);
        assert_eq!(engine.step_records()[0].frame, None);
    }
}
