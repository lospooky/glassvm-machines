/// Stage 1 — Disassembler
///
/// Decodes every aligned 16-bit word in the ROM buffer into an `Instruction`.
/// No reachability filtering here — that is the CFG builder's job.
use crate::loader::{MEM_SIZE, ROM_BASE};

/// Every logical CHIP-8 / SUPER-CHIP / XO-CHIP instruction kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    // --- 0x0 group ---
    /// 0x00E0  clear
    Cls,
    /// 0x00EE  return
    Ret,
    /// 0x00FD  exit (SUPER-CHIP)
    Exit,
    /// 0x00FE  lores (SUPER-CHIP)
    Lores,
    /// 0x00FF  hires (SUPER-CHIP)
    Hires,
    /// 0x00CN  scroll-down N (SUPER-CHIP)
    ScrollDown,
    /// 0x00DN  scroll-up N (XO-CHIP)
    ScrollUp,
    /// 0x00FB  scroll-right (SUPER-CHIP)
    ScrollRight,
    /// 0x00FC  scroll-left (SUPER-CHIP)
    ScrollLeft,
    /// 0x0NNN  raw machine call (legacy, COSMAC VIP only)
    SysCall,

    // --- 0x1–0x9 ---
    /// 0x1NNN  jump NNN
    Jump,
    /// 0x2NNN  call NNN
    Call,
    /// 0x3XNN  skip if VX == NN
    SkipEqImm,
    /// 0x4XNN  skip if VX != NN
    SkipNeImm,
    /// 0x5XY0  skip if VX == VY
    SkipEqReg,
    /// 0x5XY2  save vx - vy (XO-CHIP)
    SaveRange,
    /// 0x5XY3  load vx - vy (XO-CHIP)
    LoadRange,
    /// 0x6XNN  VX := NN
    SetImm,
    /// 0x7XNN  VX += NN (no carry)
    AddImm,
    /// 0x8XY0  VX := VY
    MovReg,
    /// 0x8XY1  VX |= VY
    Or,
    /// 0x8XY2  VX &= VY
    And,
    /// 0x8XY3  VX ^= VY
    Xor,
    /// 0x8XY4  VX += VY (carry → VF)
    AddReg,
    /// 0x8XY5  VX -= VY (borrow → VF)
    Sub,
    /// 0x8XY6  VX >>= VY (shift right; LSB → VF)
    Shr,
    /// 0x8XY7  VX =- VY  (VY-VX; borrow → VF)
    Subn,
    /// 0x8XYE  VX <<= VY (shift left; MSB → VF)
    Shl,
    /// 0x9XY0  skip if VX != VY
    SkipNeReg,

    // --- 0xA–0xF ---
    /// 0xANNN  I := NNN
    SetI,
    /// 0xBNNN  jump NNN + V0 (or VX on SCHIP)
    JumpOffset,
    /// 0xCXNN  VX := random & NN
    Rand,
    /// 0xDXYN  draw sprite at (VX,VY) height N; 0=16x16 (SUPER-CHIP)
    Draw,
    /// 0xEX9E  skip if key VX pressed
    SkipKey,
    /// 0xEXA1  skip if key VX not pressed
    SkipNoKey,
    /// 0xFX07  VX := delay
    GetDelay,
    /// 0xFX0A  VX := key (wait)
    WaitKey,
    /// 0xFX15  delay := VX
    SetDelay,
    /// 0xFX18  sound := VX
    SetSound,
    /// 0xFX1E  I += VX
    AddI,
    /// 0xFX29  I := hex font VX
    FontChar,
    /// 0xFX30  I := big hex font VX (SUPER-CHIP)
    BigFontChar,
    /// 0xFX33  BCD of VX → mem[I..I+2]
    Bcd,
    /// 0xFX55  save V0..VX to mem[I] (increments I on VIP)
    SaveRegs,
    /// 0xFX65  load V0..VX from mem[I] (increments I on VIP)
    LoadRegs,
    /// 0xFX75  save V0..VX to persistent flags (SUPER-CHIP)
    SaveFlags,
    /// 0xFX85  load V0..VX from persistent flags (SUPER-CHIP)
    LoadFlags,
    /// 0xF000 NNNN  I := long NNNN (XO-CHIP; consumes 4 bytes)
    SetILong,
    /// 0xFN01  plane select bitmask N (XO-CHIP)
    Plane,
    /// 0xF002  audio waveform from mem[I] (XO-CHIP)
    Audio,
    /// 0xFX3A  pitch := VX (XO-CHIP)
    Pitch,

    /// Unrecognized opcode
    Unknown,
}

impl OpKind {
    /// True if this instruction can unconditionally terminate forward flow.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            OpKind::Jump | OpKind::JumpOffset | OpKind::Ret | OpKind::Exit
        )
    }

    /// True if this instruction is a conditional skip (has two successors).
    pub fn is_skip(self) -> bool {
        matches!(
            self,
            OpKind::SkipEqImm
                | OpKind::SkipNeImm
                | OpKind::SkipEqReg
                | OpKind::SkipNeReg
                | OpKind::SkipKey
                | OpKind::SkipNoKey
        )
    }

    /// True if this writes into VF as a side-effect flag.
    pub fn writes_vf_as_flag(self) -> bool {
        matches!(
            self,
            OpKind::AddReg | OpKind::Sub | OpKind::Subn | OpKind::Shr | OpKind::Shl | OpKind::Draw
        )
    }

    /// True if VF is incidentally clobbered (OR/AND/XOR on COSMAC VIP).
    pub fn clobbers_vf_on_vip(self) -> bool {
        matches!(self, OpKind::Or | OpKind::And | OpKind::Xor)
    }
}

/// A decoded instruction.
#[derive(Debug, Clone)]
pub struct Instruction {
    /// Address in the 64 KiB CHIP-8-family address space.
    pub addr: u16,
    /// Raw 16-bit word (the first word; SetILong has a second word).
    pub word: u16,
    pub kind: OpKind,
    /// Second nibble — register index X (0–0xF).
    pub x: u8,
    /// Third nibble — register index Y (0–0xF).
    pub y: u8,
    /// Fourth nibble — 4-bit literal N.
    pub n: u8,
    /// Low byte — 8-bit literal NN.
    pub nn: u8,
    /// Low 12 bits — 12-bit address NNN.
    pub nnn: u16,
    /// For SetILong: the 16-bit address in the following two bytes.
    pub long_addr: Option<u16>,
    /// Byte length of this instruction (2 normally; 4 for SetILong).
    pub byte_len: u8,
}

impl Instruction {
    /// Address of the next sequential instruction as a wide exclusive value.
    ///
    /// `0xFFFE` is a valid final instruction address in XO-CHIP memory, so its
    /// successor must be representable as the out-of-space sentinel `0x10000`
    /// rather than wrapping to zero.
    pub fn next_addr(&self) -> usize {
        self.addr as usize + self.byte_len as usize
    }
}

/// Decode every aligned pair of bytes in `mem[ROM_BASE .. ROM_BASE + rom_len]`.
/// Returns one `Instruction` per even offset within the ROM.
pub fn disassemble(mem: &[u8; MEM_SIZE], rom_len: usize) -> Vec<Instruction> {
    let end = ROM_BASE + rom_len;
    let mut instrs = Vec::with_capacity(rom_len / 2);
    let mut addr = ROM_BASE;

    while addr + 1 < end {
        let word = ((mem[addr] as u16) << 8) | mem[addr + 1] as u16;
        let instr = decode(word, addr as u16, mem, end);
        let step = instr.byte_len as usize;
        instrs.push(instr);
        addr += step;
    }
    instrs
}

fn decode(word: u16, addr: u16, mem: &[u8; MEM_SIZE], end: usize) -> Instruction {
    let x = ((word >> 8) & 0xF) as u8;
    let y = ((word >> 4) & 0xF) as u8;
    let n = (word & 0xF) as u8;
    let nn = (word & 0xFF) as u8;
    let nnn = word & 0xFFF;

    let (kind, long_addr, byte_len) = decode_kind(word, x, y, n, nn, nnn, addr, mem, end);

    Instruction {
        addr,
        word,
        kind,
        x,
        y,
        n,
        nn,
        nnn,
        long_addr,
        byte_len,
    }
}

// Decoding keeps the extracted instruction fields explicit so rule arms do
// not repeatedly unpack a less-readable context object.
#[allow(clippy::too_many_arguments)]
fn decode_kind(
    word: u16,
    _x: u8,
    _y: u8,
    n: u8,
    nn: u8,
    nnn: u16,
    addr: u16,
    mem: &[u8; MEM_SIZE],
    end: usize,
) -> (OpKind, Option<u16>, u8) {
    let top = (word >> 12) & 0xF;
    match top {
        0x0 => match word {
            0x00E0 => (OpKind::Cls, None, 2),
            0x00EE => (OpKind::Ret, None, 2),
            0x00FB => (OpKind::ScrollRight, None, 2),
            0x00FC => (OpKind::ScrollLeft, None, 2),
            0x00FD => (OpKind::Exit, None, 2),
            0x00FE => (OpKind::Lores, None, 2),
            0x00FF => (OpKind::Hires, None, 2),
            _ if (word & 0xFFF0) == 0x00C0 => (OpKind::ScrollDown, None, 2),
            _ if (word & 0xFFF0) == 0x00D0 => (OpKind::ScrollUp, None, 2),
            _ => {
                if nnn != 0 {
                    (OpKind::SysCall, None, 2)
                } else {
                    (OpKind::Unknown, None, 2)
                }
            }
        },
        0x1 => (OpKind::Jump, None, 2),
        0x2 => (OpKind::Call, None, 2),
        0x3 => (OpKind::SkipEqImm, None, 2),
        0x4 => (OpKind::SkipNeImm, None, 2),
        0x5 => match n {
            0x0 => (OpKind::SkipEqReg, None, 2),
            0x2 => (OpKind::SaveRange, None, 2),
            0x3 => (OpKind::LoadRange, None, 2),
            _ => (OpKind::Unknown, None, 2),
        },
        0x6 => (OpKind::SetImm, None, 2),
        0x7 => (OpKind::AddImm, None, 2),
        0x8 => match n {
            0x0 => (OpKind::MovReg, None, 2),
            0x1 => (OpKind::Or, None, 2),
            0x2 => (OpKind::And, None, 2),
            0x3 => (OpKind::Xor, None, 2),
            0x4 => (OpKind::AddReg, None, 2),
            0x5 => (OpKind::Sub, None, 2),
            0x6 => (OpKind::Shr, None, 2),
            0x7 => (OpKind::Subn, None, 2),
            0xE => (OpKind::Shl, None, 2),
            _ => (OpKind::Unknown, None, 2),
        },
        0x9 => {
            if n == 0 {
                (OpKind::SkipNeReg, None, 2)
            } else {
                (OpKind::Unknown, None, 2)
            }
        }
        0xA => (OpKind::SetI, None, 2),
        0xB => (OpKind::JumpOffset, None, 2),
        0xC => (OpKind::Rand, None, 2),
        0xD => (OpKind::Draw, None, 2),
        0xE => match nn {
            0x9E => (OpKind::SkipKey, None, 2),
            0xA1 => (OpKind::SkipNoKey, None, 2),
            _ => (OpKind::Unknown, None, 2),
        },
        0xF => {
            // XO-CHIP: F000 NNNN — 4-byte long address load
            if word == 0xF000 {
                let next = addr as usize + 2;
                if next + 1 < end {
                    let lo = ((mem[next] as u16) << 8) | mem[next + 1] as u16;
                    return (OpKind::SetILong, Some(lo), 4);
                } else {
                    return (OpKind::SetILong, None, 4); // truncated
                }
            }
            // XO-CHIP: FN01 plane select (x == 0 is also valid: plane 0)
            if nn == 0x01 {
                return (OpKind::Plane, None, 2);
            }
            // XO-CHIP: F002 audio
            if word == 0xF002 {
                return (OpKind::Audio, None, 2);
            }
            match nn {
                0x07 => (OpKind::GetDelay, None, 2),
                0x0A => (OpKind::WaitKey, None, 2),
                0x15 => (OpKind::SetDelay, None, 2),
                0x18 => (OpKind::SetSound, None, 2),
                0x1E => (OpKind::AddI, None, 2),
                0x29 => (OpKind::FontChar, None, 2),
                0x30 => (OpKind::BigFontChar, None, 2),
                0x33 => (OpKind::Bcd, None, 2),
                0x3A => (OpKind::Pitch, None, 2),
                0x55 => (OpKind::SaveRegs, None, 2),
                0x65 => (OpKind::LoadRegs, None, 2),
                0x75 => (OpKind::SaveFlags, None, 2),
                0x85 => (OpKind::LoadFlags, None, 2),
                _ => (OpKind::Unknown, None, 2),
            }
        }
        _ => (OpKind::Unknown, None, 2),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal 4-kB memory with two bytes placed at ROM_BASE + `offset`.
    fn mem_with(offset: usize, hi: u8, lo: u8) -> [u8; MEM_SIZE] {
        let mut m = [0u8; MEM_SIZE];
        m[ROM_BASE + offset] = hi;
        m[ROM_BASE + offset + 1] = lo;
        m
    }

    /// Disassemble a single instruction given by `(hi, lo)` bytes.
    fn single(hi: u8, lo: u8) -> Instruction {
        let m = mem_with(0, hi, lo);
        let instrs = disassemble(&m, 2);
        assert_eq!(instrs.len(), 1);
        instrs.into_iter().next().unwrap()
    }

    // --- opcode decoding ---

    #[test]
    fn decode_cls() {
        assert_eq!(single(0x00, 0xE0).kind, OpKind::Cls);
    }

    #[test]
    fn decode_ret() {
        assert_eq!(single(0x00, 0xEE).kind, OpKind::Ret);
    }

    #[test]
    fn decode_jump() {
        let i = single(0x13, 0x45);
        assert_eq!(i.kind, OpKind::Jump);
        assert_eq!(i.nnn, 0x345);
    }

    #[test]
    fn decode_call() {
        let i = single(0x24, 0x56);
        assert_eq!(i.kind, OpKind::Call);
        assert_eq!(i.nnn, 0x456);
    }

    #[test]
    fn decode_skip_eq_imm() {
        let i = single(0x3A, 0xFF);
        assert_eq!(i.kind, OpKind::SkipEqImm);
        assert_eq!(i.x, 0xA);
        assert_eq!(i.nn, 0xFF);
    }

    #[test]
    fn decode_set_imm() {
        let i = single(0x62, 0x10);
        assert_eq!(i.kind, OpKind::SetImm);
        assert_eq!(i.x, 2);
        assert_eq!(i.nn, 0x10);
    }

    #[test]
    fn decode_draw() {
        let i = single(0xD1, 0x25);
        assert_eq!(i.kind, OpKind::Draw);
        assert_eq!(i.x, 1);
        assert_eq!(i.y, 2);
        assert_eq!(i.n, 5);
    }

    #[test]
    fn decode_add_reg() {
        let i = single(0x81, 0x24);
        assert_eq!(i.kind, OpKind::AddReg);
        assert_eq!(i.x, 1);
        assert_eq!(i.y, 2);
    }

    #[test]
    fn decode_shr_vip_style() {
        // 8XY6: Shr — x=1, y=0 (typical VIP-style encoding)
        let i = single(0x81, 0x06);
        assert_eq!(i.kind, OpKind::Shr);
        assert_eq!(i.x, 1);
        assert_eq!(i.y, 0);
    }

    #[test]
    fn decode_set_i() {
        let i = single(0xA2, 0xAA);
        assert_eq!(i.kind, OpKind::SetI);
        assert_eq!(i.nnn, 0x2AA);
    }

    #[test]
    fn decode_rand() {
        let i = single(0xC3, 0x0F);
        assert_eq!(i.kind, OpKind::Rand);
        assert_eq!(i.x, 3);
        assert_eq!(i.nn, 0x0F);
    }

    #[test]
    fn decode_skip_key() {
        let i = single(0xE5, 0x9E);
        assert_eq!(i.kind, OpKind::SkipKey);
        assert_eq!(i.x, 5);
    }

    #[test]
    fn decode_skip_no_key() {
        assert_eq!(single(0xE5, 0xA1).kind, OpKind::SkipNoKey);
    }

    #[test]
    fn decode_font_char() {
        assert_eq!(single(0xF1, 0x29).kind, OpKind::FontChar);
    }

    #[test]
    fn decode_bcd() {
        assert_eq!(single(0xF2, 0x33).kind, OpKind::Bcd);
    }

    #[test]
    fn decode_save_regs() {
        assert_eq!(single(0xF3, 0x55).kind, OpKind::SaveRegs);
    }

    #[test]
    fn decode_load_regs() {
        assert_eq!(single(0xF3, 0x65).kind, OpKind::LoadRegs);
    }

    // --- SUPER-CHIP ---

    #[test]
    fn decode_hires() {
        assert_eq!(single(0x00, 0xFF).kind, OpKind::Hires);
    }

    #[test]
    fn decode_lores() {
        assert_eq!(single(0x00, 0xFE).kind, OpKind::Lores);
    }

    #[test]
    fn decode_scroll_down() {
        let i = single(0x00, 0xC4);
        assert_eq!(i.kind, OpKind::ScrollDown);
        assert_eq!(i.n, 4);
    }

    #[test]
    fn decode_big_font() {
        assert_eq!(single(0xF1, 0x30).kind, OpKind::BigFontChar);
    }

    // --- XO-CHIP ---

    #[test]
    fn decode_scroll_up() {
        let i = single(0x00, 0xD3);
        assert_eq!(i.kind, OpKind::ScrollUp);
        assert_eq!(i.n, 3);
    }

    #[test]
    fn decode_set_i_long() {
        // F000 followed by ABCD — 4-byte instruction
        let mut m = [0u8; MEM_SIZE];
        m[ROM_BASE] = 0xF0;
        m[ROM_BASE + 1] = 0x00;
        m[ROM_BASE + 2] = 0xAB;
        m[ROM_BASE + 3] = 0xCD;
        let instrs = disassemble(&m, 4);
        assert_eq!(instrs.len(), 1, "SetILong should consume 4 bytes");
        assert_eq!(instrs[0].kind, OpKind::SetILong);
        assert_eq!(instrs[0].long_addr, Some(0xABCD));
        assert_eq!(instrs[0].byte_len, 4);
    }

    #[test]
    fn decode_plane() {
        let i = single(0xF2, 0x01);
        assert_eq!(i.kind, OpKind::Plane);
    }

    #[test]
    fn decode_audio() {
        assert_eq!(single(0xF0, 0x02).kind, OpKind::Audio);
    }

    #[test]
    fn decode_pitch() {
        assert_eq!(single(0xF5, 0x3A).kind, OpKind::Pitch);
    }

    #[test]
    fn decode_save_range() {
        let i = single(0x52, 0x42);
        assert_eq!(i.kind, OpKind::SaveRange);
    }

    #[test]
    fn decode_load_range() {
        let i = single(0x52, 0x43);
        assert_eq!(i.kind, OpKind::LoadRange);
    }

    #[test]
    fn unknown_opcode() {
        // 0x5XY1 is not a valid opcode
        assert_eq!(single(0x51, 0x21).kind, OpKind::Unknown);
    }

    // --- field extraction ---

    #[test]
    fn next_addr_normal() {
        let i = single(0x00, 0xE0);
        assert_eq!(i.next_addr(), ROM_BASE + 2);
    }

    #[test]
    fn next_addr_long() {
        let mut m = [0u8; MEM_SIZE];
        m[ROM_BASE] = 0xF0;
        m[ROM_BASE + 1] = 0x00;
        m[ROM_BASE + 2] = 0x12;
        m[ROM_BASE + 3] = 0x34;
        let instrs = disassemble(&m, 4);
        assert_eq!(instrs[0].next_addr(), ROM_BASE + 4);
    }

    #[test]
    fn next_addr_preserves_the_end_of_xochip_memory_sentinel() {
        let mut i = single(0x00, 0xE0);
        i.addr = 0xFFFE;
        assert_eq!(i.next_addr(), 0x10000);
    }

    // --- OpKind predicates ---

    #[test]
    fn is_skip_for_skip_ops() {
        assert!(OpKind::SkipEqImm.is_skip());
        assert!(OpKind::SkipNeImm.is_skip());
        assert!(OpKind::SkipEqReg.is_skip());
        assert!(OpKind::SkipNeReg.is_skip());
        assert!(OpKind::SkipKey.is_skip());
        assert!(OpKind::SkipNoKey.is_skip());
    }

    #[test]
    fn is_not_skip_for_others() {
        assert!(!OpKind::Jump.is_skip());
        assert!(!OpKind::Draw.is_skip());
        assert!(!OpKind::Ret.is_skip());
    }

    #[test]
    fn writes_vf_as_flag_set() {
        assert!(OpKind::AddReg.writes_vf_as_flag());
        assert!(OpKind::Sub.writes_vf_as_flag());
        assert!(OpKind::Subn.writes_vf_as_flag());
        assert!(OpKind::Shr.writes_vf_as_flag());
        assert!(OpKind::Shl.writes_vf_as_flag());
        assert!(OpKind::Draw.writes_vf_as_flag());
    }

    #[test]
    fn clobbers_vf_on_vip_set() {
        assert!(OpKind::Or.clobbers_vf_on_vip());
        assert!(OpKind::And.clobbers_vf_on_vip());
        assert!(OpKind::Xor.clobbers_vf_on_vip());
        assert!(!OpKind::AddReg.clobbers_vf_on_vip());
    }

    // --- multi-instruction sequences ---

    #[test]
    fn disassemble_sequence() {
        // 6000 (set v0,0)  1200 (jump 0x200)
        let mut m = [0u8; MEM_SIZE];
        m[ROM_BASE] = 0x60;
        m[ROM_BASE + 1] = 0x00;
        m[ROM_BASE + 2] = 0x12;
        m[ROM_BASE + 3] = 0x00;
        let instrs = disassemble(&m, 4);
        assert_eq!(instrs.len(), 2);
        assert_eq!(instrs[0].kind, OpKind::SetImm);
        assert_eq!(instrs[1].kind, OpKind::Jump);
        assert_eq!(instrs[0].addr, 0x200);
        assert_eq!(instrs[1].addr, 0x202);
    }
}
