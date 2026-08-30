use crate::machine::cpu::{KeyWait, MEM_SIZE};
use crate::machine::display::BUF_SIZE;

/// A point-in-time snapshot of the complete CHIP-8 machine state.
///
/// Created via [`crate::cpu::Cpu::save_snapshot`] and restored via
/// [`crate::cpu::Cpu::load_snapshot`]. Includes the RNG state so that
/// execution resumes deterministically from the snapshot point.
///
/// Large buffers are heap-allocated to avoid stack overflow.
#[derive(Clone)]
pub struct Snapshot {
    pub mem: Box<[u8; MEM_SIZE]>,
    pub v: [u8; 16],
    pub i: u16,
    pub pc: u16,
    pub stack: [u16; 16],
    pub sp: u8,
    pub dt: u8,
    pub st: u8,
    /// Display bitplane buffers: two planes × (128×64) pixels.
    pub display_buf: Box<[[u8; BUF_SIZE]; 2]>,
    pub display_hires: bool,
    pub display_plane: u8,
    /// Key state packed as a bitmask: bit N = key N is currently pressed.
    pub keys: u16,
    /// Internal xorshift64 state for the per-instance RNG.
    pub rng_state: u64,
    pub flags: [u8; 16],
    pub audio_buf: [u8; 16],
    pub audio_pitch: u8,
    pub halted: bool,
    pub key_wait: KeyWait,
}
