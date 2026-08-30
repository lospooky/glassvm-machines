//! Private mirror of the reviewed CUDA C ABI.

use cudarc::driver::{DeviceRepr, ValidAsZeroBits};
use std::mem::offset_of;

pub(super) const ABI_VERSION: u32 = 1;
pub(super) const KERNEL_SYMBOL: &str = "chip8_cuda_run_v1";
pub(super) const RUN_FRAMES: u32 = 0;
pub(super) const RUN_CYCLES: u32 = 1;

pub(super) const STATUS_TIMEOUT: u32 = 0;
pub(super) const STATUS_HALTED: u32 = 1;
pub(super) const STATUS_WAITING_FOR_INPUT: u32 = 2;
pub(super) const STATUS_INVALID_OPCODE: u32 = 3;
pub(super) const STATUS_STACK_OVERFLOW: u32 = 4;
pub(super) const STATUS_STACK_UNDERFLOW: u32 = 5;
pub(super) const STATUS_MEMORY_FAULT: u32 = 6;
pub(super) const STATUS_INVALID_CONFIGURATION: u32 = 7;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct RawConfig {
    pub abi_version: u32,
    pub lane_count: u32,
    pub run_mode: u32,
    pub max_frames: u32,
    pub max_cycles: u64,
    pub cycles_per_frame: u32,
    pub input_frame_count: u32,
    pub input_stride: u32,
    pub frame_hash_capacity: u32,
    pub frame_hash_stride: u32,
    pub quirk_flags: u32,
    pub reserved: u32,
    pub reserved_tail: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct RawLaneState {
    pub rng_state: u64,
    pub i: u16,
    pub pc: u16,
    pub stack: [u16; 16],
    pub keys: u16,
    pub v: [u8; 16],
    pub flags: [u8; 16],
    pub audio_buf: [u8; 16],
    pub sp: u8,
    pub dt: u8,
    pub st: u8,
    pub hires: u8,
    pub plane: u8,
    pub halted: u8,
    pub key_wait_kind: u8,
    pub key_wait_reg: u8,
    pub key_wait_key: u8,
    pub audio_pitch: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct RawLaneResult {
    pub abi_version: u32,
    pub status: u32,
    pub cycles_executed: u64,
    pub frames_executed: u32,
    pub frame_hashes_written: u32,
    pub fault_address: u32,
    pub invalid_opcode: u16,
    pub reserved: u16,
    pub final_frame_hash: u64,
    pub draw_count: u32,
    pub collision_count: u32,
    pub input_opcode_count: u32,
    pub clear_count: u32,
    pub delay_timer_set_count: u32,
    pub delay_timer_nonzero_count: u32,
    pub sound_timer_set_count: u32,
    pub sound_timer_nonzero_count: u32,
    pub scroll_count: u32,
    pub opcode_counts: [u32; 16],
    pub reserved_counts: u32,
    pub final_state: RawLaneState,
}

// SAFETY: all three records are `repr(C)`, contain only fixed-width integer
// fields/arrays, and are compile-time checked against kernel.cu below.
unsafe impl DeviceRepr for RawConfig {}
unsafe impl DeviceRepr for RawLaneState {}
unsafe impl DeviceRepr for RawLaneResult {}
// SAFETY: every field is an integer or integer array and all-zero is a valid
// pre-launch representation. The kernel overwrites every published field.
unsafe impl ValidAsZeroBits for RawLaneResult {}

const _: () = {
    assert!(size_of::<RawConfig>() == 56);
    assert!(align_of::<RawConfig>() == 8);
    assert!(size_of::<RawLaneState>() == 104);
    assert!(align_of::<RawLaneState>() == 8);
    assert!(size_of::<RawLaneResult>() == 248);
    assert!(align_of::<RawLaneResult>() == 8);
    assert!(offset_of!(RawConfig, max_cycles) == 16);
    assert!(offset_of!(RawConfig, reserved_tail) == 52);
    assert!(offset_of!(RawLaneResult, final_frame_hash) == 32);
    assert!(offset_of!(RawLaneResult, draw_count) == 40);
    assert!(offset_of!(RawLaneResult, opcode_counts) == 76);
    assert!(offset_of!(RawLaneResult, reserved_counts) == 140);
    assert!(offset_of!(RawLaneResult, final_state) == 144);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_abi_layout_is_frozen() {
        assert_eq!(size_of::<RawConfig>(), 56);
        assert_eq!(size_of::<RawLaneState>(), 104);
        assert_eq!(size_of::<RawLaneResult>(), 248);
        assert_eq!(offset_of!(RawConfig, reserved_tail), 52);
        assert_eq!(offset_of!(RawLaneResult, reserved_counts), 140);
        assert_eq!(offset_of!(RawLaneResult, final_state), 144);
    }
}
