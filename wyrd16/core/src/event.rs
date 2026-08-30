//! Native effects produced by one rune transition.

use crate::Rune;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeEffectKind {
    InstructionDecoded,
    RunHalted,
    RegisterWrite,
    MemoryRead,
    MemoryWrite,
    BranchTaken,
    DisplayWrite,
    InputSampled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepEffect {
    pub pc: u16,
    pub next_pc: u16,
    pub rune: Rune,
    pub kind: NativeEffectKind,
    pub register_write: Option<(u8, u8, u8)>,
    pub memory_access: Option<(bool, u16, u8, u8)>,
    pub display_writes: usize,
    pub palette_write: Option<(u8, u8, u8)>,
    pub input_sample: Option<u8>,
    pub random_sample: Option<u8>,
    pub branch_taken: bool,
}
