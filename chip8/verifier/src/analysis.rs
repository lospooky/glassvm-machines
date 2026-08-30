/// Stage 3 — Analysis Passes
///
/// Four passes run over the CFG's reachable instruction set:
///
///   Pass A — Validity:         unknown opcodes, bad calls, raw machine calls
///   Pass B — Classification:   determine minimum required extension level
///   Pass C — Quirk sensitivity: flag instructions with platform-dependent behavior
///   Pass D — Register hazards: VF used as both flag output and data input
use crate::cfg::Cfg;
use crate::diagnostic::Diagnostic;
use crate::disasm::{Instruction, OpKind};
use crate::metrics::{BehavioralPriors, StructuralMetrics, ValidityFlags};

/// The minimum CHIP-8 extension required to run this ROM.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExtensionLevel {
    Chip8,
    SuperChip,
    XoChip,
}

impl std::fmt::Display for ExtensionLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtensionLevel::Chip8 => write!(f, "CHIP-8"),
            ExtensionLevel::SuperChip => write!(f, "SUPER-CHIP"),
            ExtensionLevel::XoChip => write!(f, "XO-CHIP"),
        }
    }
}

pub struct AnalysisResult {
    pub diags: Vec<Diagnostic>,
    pub extension: ExtensionLevel,
    /// Names of specific extension instructions found (for the summary).
    pub extension_instructions: Vec<String>,
    pub validity: ValidityFlags,
    pub structural: StructuralMetrics,
    pub behavioral: BehavioralPriors,
}

pub fn run(cfg: &Cfg, instrs: &[Instruction], rom_len: usize) -> AnalysisResult {
    let mut diags: Vec<Diagnostic> = Vec::new();
    let mut extension = ExtensionLevel::Chip8;
    let mut ext_instrs: Vec<String> = Vec::new();

    // Sort reachable instructions by address for deterministic output.
    let mut sorted: Vec<_> = cfg.reachable.values().collect();
    sorted.sort_by_key(|i| i.addr);

    // -------------------------------------------------------------------------
    // Pass A — Validity
    // -------------------------------------------------------------------------
    for instr in &sorted {
        match instr.kind {
            OpKind::Unknown => {
                diags.push(Diagnostic::error(
                    "E004",
                    instr.addr,
                    format!("Unknown opcode 0x{:04X} in reachable code", instr.word),
                ));
            }
            OpKind::SysCall => {
                diags.push(Diagnostic::error(
                    "E008",
                    instr.addr,
                    format!(
                        "Raw machine call 0x{:04X} (0NNN) — only valid on original COSMAC VIP hardware",
                        instr.word
                    ),
                ));
            }
            _ => {}
        }
    }

    // -------------------------------------------------------------------------
    // Pass B — Extension Classification
    // -------------------------------------------------------------------------
    for instr in &sorted {
        let (req, name) = required_extension(instr.kind, instr.x, instr.word);
        if let Some(lvl) = req {
            if lvl > extension {
                extension = lvl;
            }
            if let Some(n) = name {
                let entry = format!("0x{:03X}  {n}", instr.addr);
                if !ext_instrs.contains(&entry) {
                    ext_instrs.push(entry);
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // Pass C — Quirk Sensitivity
    // -------------------------------------------------------------------------
    for instr in sorted.iter() {
        match instr.kind {
            // W002 — shift quirk: X != Y means behavior differs
            OpKind::Shr | OpKind::Shl if instr.x != instr.y => {
                diags.push(Diagnostic::warning(
                    "W002",
                    instr.addr,
                    format!(
                        "Shift instruction 0x{:04X}: X({}) != Y({}) — behavior differs between VIP (VX=VY>>N) and SUPER-CHIP (VX=VX>>N)",
                        instr.word, instr.x, instr.y
                    ),
                ));
            }

            // W003 — FX55/FX65 I post-increment
            OpKind::SaveRegs | OpKind::LoadRegs => {
                // Heuristic: check if the next reachable instruction re-initializes I.
                // If it doesn't, warn.
                let reinit = u16::try_from(instr.next_addr())
                    .ok()
                    .and_then(|next_addr| cfg.reachable.get(&next_addr))
                    .is_some_and(|next| {
                        matches!(
                            next.kind,
                            OpKind::SetI
                                | OpKind::SetILong
                                | OpKind::FontChar
                                | OpKind::BigFontChar
                        )
                    });
                if !reinit {
                    diags.push(Diagnostic::warning(
                        "W003",
                        instr.addr,
                        format!(
                            "0x{:04X} ({}): I is incremented after this on VIP but unchanged on SUPER-CHIP — re-initialize I before next use for compatibility",
                            instr.word,
                            if instr.kind == OpKind::SaveRegs { "save" } else { "load" },
                        ),
                    ));
                }
            }

            // W004 — BNNN jump offset register
            OpKind::JumpOffset => {
                let x = (instr.nnn >> 8) as u8;
                if x != 0 {
                    diags.push(Diagnostic::warning(
                        "W004",
                        instr.addr,
                        format!(
                            "0x{:04X} BNNN: upper nibble is {x} (non-zero) — on VIP the offset is always V0; on SUPER-CHIP it uses V{x}",
                            instr.word
                        ),
                    ));
                } else {
                    // Even B0NN is worth flagging since SCHIP still reads V0+NNN vs V0+0NN.
                    diags.push(Diagnostic::warning(
                        "W004",
                        instr.addr,
                        format!(
                            "0x{:04X} BNNN: jump-with-offset behavior differs (V0+NNN on VIP vs VX+XNN on SUPER-CHIP)",
                            instr.word
                        ),
                    ));
                }
            }

            // W006 — OR/AND/XOR followed by a VF read
            kind if kind.clobbers_vf_on_vip() => {
                if let Ok(next_addr) = u16::try_from(instr.next_addr())
                    && let Some(next) = cfg.reachable.get(&next_addr)
                    && reads_vf(next)
                {
                    diags.push(Diagnostic::warning(
                        "W006",
                        instr.addr,
                        format!(
                            "0x{:04X} ({}) followed by instruction at 0x{:03X} that reads VF — VF is reset to 0 after OR/AND/XOR on VIP but unchanged on SUPER-CHIP",
                            instr.word,
                            op_name(kind),
                            next_addr
                        ),
                    ));
                }
            }

            _ => {}
        }
    }

    // -------------------------------------------------------------------------
    // Pass D — Register Hazards
    // -------------------------------------------------------------------------
    for instr in sorted.iter() {
        // W007 — VF used as both flag output and data input in same block
        if instr.kind.writes_vf_as_flag() || instr.kind.clobbers_vf_on_vip() {
            if let Ok(next_addr) = u16::try_from(instr.next_addr())
                && let Some(next) = cfg.reachable.get(&next_addr)
                && reads_vf(next)
                && next.kind.writes_vf_as_flag()
            {
                diags.push(Diagnostic::warning(
                    "W007",
                    instr.addr,
                    format!(
                        "VF flag from 0x{:04X} is immediately consumed by another flag-writing instruction at 0x{:03X} — possible unintended flag clobber",
                        instr.word, next_addr
                    ),
                ));
            }
        }

        // W008 — VF directly written by program (intentional override)
        match instr.kind {
            OpKind::SetImm if instr.x == 0xF => {
                diags.push(Diagnostic::info(
                    "W008",
                    instr.addr,
                    format!(
                        "0x{:04X}: VF explicitly set to 0x{:02X} — overrides flag register",
                        instr.word, instr.nn
                    ),
                ));
            }
            OpKind::MovReg if instr.x == 0xF => {
                diags.push(Diagnostic::info(
                    "W008",
                    instr.addr,
                    format!(
                        "0x{:04X}: VF := V{} — explicit write to flag register",
                        instr.word, instr.y
                    ),
                ));
            }
            _ => {}
        }
    }

    // -------------------------------------------------------------------------
    // Pass E — Structural metrics
    // -------------------------------------------------------------------------
    let structural = pass_e(cfg, rom_len);

    // -------------------------------------------------------------------------
    // Pass F — Behavioral priors
    // -------------------------------------------------------------------------
    let behavioral = pass_f(cfg);

    // -------------------------------------------------------------------------
    // Pass G — Validity flags
    // -------------------------------------------------------------------------
    let validity = pass_g(cfg, instrs, &diags);

    AnalysisResult {
        diags,
        extension,
        extension_instructions: ext_instrs,
        validity,
        structural,
        behavioral,
    }
}

// ---------------------------------------------------------------------------
// Pass E — Structural metrics
// ---------------------------------------------------------------------------

fn pass_e(cfg: &Cfg, rom_len: usize) -> StructuralMetrics {
    let reachable_instruction_count = cfg.reachable.len();
    let _total_count = if rom_len == 0 { 1 } else { rom_len / 2 }; // avoid div-by-zero

    let estimated_code_bytes: usize = cfg.reachable.values().map(|i| i.byte_len as usize).sum();
    let estimated_data_bytes = rom_len.saturating_sub(estimated_code_bytes);

    let reachable_ratio = reachable_instruction_count as f64 / (rom_len.max(2) / 2) as f64;

    StructuralMetrics {
        reachable_instruction_count,
        basic_block_count: cfg.basic_blocks.len(),
        loop_count: cfg.loop_count,
        max_cfg_depth: cfg.max_cfg_depth,
        reachable_ratio,
        estimated_code_bytes,
        estimated_data_bytes,
    }
}

// ---------------------------------------------------------------------------
// Pass F — Behavioral priors
// ---------------------------------------------------------------------------

fn pass_f(cfg: &Cfg) -> BehavioralPriors {
    let mut priors = BehavioralPriors::default();

    for instr in cfg.reachable.values() {
        match instr.kind {
            OpKind::Draw => priors.contains_draw = true,
            OpKind::SkipKey | OpKind::SkipNoKey | OpKind::WaitKey => {
                priors.contains_key_input = true;
            }
            OpKind::SetDelay | OpKind::GetDelay => priors.contains_timers = true,
            OpKind::SetSound | OpKind::Pitch | OpKind::Audio => {
                priors.contains_sound = true;
            }
            OpKind::Rand => priors.contains_randomness = true,
            _ => {}
        }
    }

    // Collision detection: within each basic block, DRW immediately followed
    // by an instruction that reads VF.
    if !priors.contains_collision_detection {
        'outer: for block in cfg.basic_blocks.values() {
            let addrs = &block.addrs;
            for window in addrs.windows(2) {
                let (a, b) = (window[0], window[1]);
                if let (Some(ia), Some(ib)) = (cfg.reachable.get(&a), cfg.reachable.get(&b))
                    && ia.kind == OpKind::Draw
                    && reads_vf(ib)
                {
                    priors.contains_collision_detection = true;
                    break 'outer;
                }
            }
        }
    }

    priors
}

// ---------------------------------------------------------------------------
// Pass G — Validity flags
// ---------------------------------------------------------------------------

fn pass_g(cfg: &Cfg, instrs: &[Instruction], diags: &[Diagnostic]) -> ValidityFlags {
    // valid_opcode_ratio: proportion of words in the full linear disassembly
    // that decoded to a known opcode.
    let total = instrs.len();
    let unknown_count = instrs.iter().filter(|i| i.kind == OpKind::Unknown).count();
    let valid_opcode_ratio = if total == 0 {
        1.0
    } else {
        (total - unknown_count) as f64 / total as f64
    };

    let has_illegal_opcode = diags.iter().any(|d| d.code == "E004");

    let bound_codes = ["E003", "E005", "E006", "E007"];
    let cfg_well_formed = !diags.iter().any(|d| bound_codes.contains(&d.code.as_str()));
    let out_of_bounds_jump_count = diags
        .iter()
        .filter(|d| bound_codes.contains(&d.code.as_str()))
        .count() as u32;

    // Stack overflow: call depth > 12 (original VIP hardware limit) or recursion.
    let stack_overflow_risk = cfg.max_call_depth > 12 || cfg.has_recursive_call;

    // Stack underflow: a RET is reachable but there are no recorded call-return
    // sites (i.e. no CALL instruction was seen, so the stack would be empty).
    let has_ret = cfg.reachable.values().any(|i| i.kind == OpKind::Ret);
    let stack_underflow_risk = has_ret && cfg.return_sites.is_empty();

    // Out-of-bounds memory references: look for SetI / SetILong followed
    // immediately (at addr+2 / addr+4) by a dereference instruction; if the
    // loaded address + access width exceeds the address space, count it.
    let out_of_bounds_mem_ref_count = count_oob_mem_refs(cfg);

    ValidityFlags {
        valid_opcode_ratio,
        has_illegal_opcode,
        cfg_well_formed,
        stack_underflow_risk,
        stack_overflow_risk,
        out_of_bounds_jump_count,
        out_of_bounds_mem_ref_count,
        has_side_effect_in_loop: detect_side_effect_in_loop(cfg),
        has_any_skip: cfg.reachable.values().any(|i| i.kind.is_skip()),
    }
}

/// Returns `true` if any basic block that contains a backward branch (loop
/// body) also contains an instruction with a visible side effect.
///
/// "Backward branch" means the block has a successor whose leader address is
/// ≤ the current block's leader — i.e. the block ends with a jump back.
/// Side-effect opcodes: Draw, Cls, SetDelay, SetSound, Rand, Audio, Pitch.
fn detect_side_effect_in_loop(cfg: &Cfg) -> bool {
    use OpKind::*;

    fn is_side_effect(k: OpKind) -> bool {
        matches!(
            k,
            Draw | Cls
                | SetDelay
                | SetSound
                | Rand
                | Audio
                | Pitch
                | ScrollDown
                | ScrollUp
                | ScrollLeft
                | ScrollRight
        )
    }

    for block in cfg.basic_blocks.values() {
        // Is this a loop block? (any successor jumps back)
        let in_loop = block.successors.iter().any(|&succ| succ <= block.leader);

        if !in_loop {
            continue;
        }

        // Check every instruction in this block for a side effect.
        for &addr in &block.addrs {
            if let Some(instr) = cfg.reachable.get(&addr)
                && is_side_effect(instr.kind)
            {
                return true;
            }
        }
    }

    false
}

/// Scan `count_oob_mem_refs` (SetI / SetILong) → dereference pairs where the
/// computed end address falls outside the XO-CHIP 64 KiB address space.
fn count_oob_mem_refs(cfg: &Cfg) -> u32 {
    let mut count = 0u32;

    for instr in cfg.reachable.values() {
        let i_val: Option<u16> = match instr.kind {
            OpKind::SetI => Some(instr.nnn),
            OpKind::SetILong => instr.long_addr,
            _ => None,
        };
        let Some(i_val) = i_val else { continue };

        // The instruction immediately after this one (by address).
        let Ok(next_addr) = u16::try_from(instr.next_addr()) else {
            continue;
        };
        let Some(next) = cfg.reachable.get(&next_addr) else {
            continue;
        };

        let access_width: u16 = match next.kind {
            OpKind::SaveRegs | OpKind::LoadRegs => (next.x as u16) + 1,
            OpKind::Bcd => 3,
            OpKind::Draw => {
                if next.n == 0 {
                    32
                } else {
                    next.n as u16
                }
            }
            OpKind::Audio => 16,
            _ => continue,
        };

        // Check wraparound / out-of-64-KiB-space without saturating u16.
        if u32::from(i_val) + u32::from(access_width) > crate::loader::MEM_SIZE as u32 {
            count += 1;
        }
    }

    count
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns the extension level required by this opkind, and a display name.
fn required_extension(
    kind: OpKind,
    x: u8,
    word: u16,
) -> (Option<ExtensionLevel>, Option<&'static str>) {
    match kind {
        // SUPER-CHIP
        OpKind::Hires => (Some(ExtensionLevel::SuperChip), Some("hires (00FF)")),
        OpKind::Lores => (Some(ExtensionLevel::SuperChip), Some("lores (00FE)")),
        OpKind::Exit => (Some(ExtensionLevel::SuperChip), Some("exit (00FD)")),
        OpKind::ScrollDown => (Some(ExtensionLevel::SuperChip), Some("scroll-down (00CN)")),
        OpKind::ScrollRight => (Some(ExtensionLevel::SuperChip), Some("scroll-right (00FB)")),
        OpKind::ScrollLeft => (Some(ExtensionLevel::SuperChip), Some("scroll-left (00FC)")),
        OpKind::BigFontChar => (Some(ExtensionLevel::SuperChip), Some("bighex (FX30)")),
        OpKind::Draw if (word & 0xF) == 0 => {
            (Some(ExtensionLevel::SuperChip), Some("16×16 sprite (DXY0)"))
        }
        OpKind::SaveFlags | OpKind::LoadFlags if x <= 7 => (
            Some(ExtensionLevel::SuperChip),
            Some("saveflags/loadflags (FX75/FX85)"),
        ),

        // XO-CHIP
        OpKind::ScrollUp => (Some(ExtensionLevel::XoChip), Some("scroll-up (00DN)")),
        OpKind::SaveRange => (Some(ExtensionLevel::XoChip), Some("range save (5XY2)")),
        OpKind::LoadRange => (Some(ExtensionLevel::XoChip), Some("range load (5XY3)")),
        OpKind::SetILong => (
            Some(ExtensionLevel::XoChip),
            Some("long address (F000 NNNN)"),
        ),
        OpKind::Plane => (Some(ExtensionLevel::XoChip), Some("plane select (FN01)")),
        OpKind::Audio => (Some(ExtensionLevel::XoChip), Some("audio waveform (F002)")),
        OpKind::Pitch => (Some(ExtensionLevel::XoChip), Some("pitch (FX3A)")),
        OpKind::SaveFlags | OpKind::LoadFlags => {
            // x > 7 means XO-CHIP extended range
            (
                Some(ExtensionLevel::XoChip),
                Some("saveflags/loadflags x>7 (FX75/FX85)"),
            )
        }

        _ => (None, None),
    }
}

/// Returns true if the instruction uses the value of VF as an input operand
/// (not just as a destination).
fn reads_vf(instr: &crate::disasm::Instruction) -> bool {
    use OpKind::*;
    match instr.kind {
        // Any ALU op where X or Y == 0xF
        AddReg | Sub | Subn | Or | And | Xor | Shr | Shl | MovReg => {
            instr.x == 0xF || instr.y == 0xF
        }
        // Skip-if-reg where X or Y == 0xF
        SkipEqReg | SkipNeReg => instr.x == 0xF || instr.y == 0xF,
        // Skip-if-imm where X == 0xF
        SkipEqImm | SkipNeImm => instr.x == 0xF,
        // AddImm on VF
        AddImm => instr.x == 0xF,
        // Font/Save/Load/Draw using VF as position/height
        FontChar | BigFontChar => instr.x == 0xF,
        Draw => instr.x == 0xF || instr.y == 0xF,
        _ => false,
    }
}

fn op_name(kind: OpKind) -> &'static str {
    match kind {
        OpKind::Or => "OR",
        OpKind::And => "AND",
        OpKind::Xor => "XOR",
        _ => "op",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfg;
    use crate::disasm::disassemble;
    use crate::loader::{MEM_SIZE, ROM_BASE};

    /// Build a `Cfg` + run analysis from raw ROM bytes.
    fn analyse(bytes: &[u8]) -> AnalysisResult {
        let mut mem = [0u8; MEM_SIZE];
        let len = bytes.len().min(MEM_SIZE - ROM_BASE);
        mem[ROM_BASE..ROM_BASE + len].copy_from_slice(&bytes[..len]);
        let instrs = disassemble(&mem, len);
        let rom_end = ROM_BASE + len;
        let cfg = cfg::build(&instrs, rom_end);
        run(&cfg, &instrs, len)
    }

    fn codes(r: &AnalysisResult) -> Vec<&str> {
        r.diags.iter().map(|d| d.code.as_str()).collect()
    }

    // -----------------------------------------------------------------------
    // Pass A — Validity
    // -----------------------------------------------------------------------

    #[test]
    fn unknown_opcode_emits_e004() {
        // 0x5XY1 is not a valid opcode — should be reachable and flagged
        let bytes = [0x51, 0x21]; // 0x5121: unknown
        let r = analyse(&bytes);
        assert!(codes(&r).contains(&"E004"), "{:?}", r.diags);
    }

    #[test]
    fn syscall_emits_e008() {
        // 0x0123 — raw machine call
        let bytes = [0x01, 0x23];
        let r = analyse(&bytes);
        assert!(codes(&r).contains(&"E008"), "{:?}", r.diags);
    }

    #[test]
    fn clean_chip8_rom_no_validity_errors() {
        // 00E0 (cls) 1200 (jump self)
        let r = analyse(&[0x00, 0xE0, 0x12, 0x00]);
        assert!(!codes(&r).contains(&"E004"));
        assert!(!codes(&r).contains(&"E008"));
    }

    // -----------------------------------------------------------------------
    // Pass B — Classification
    // -----------------------------------------------------------------------

    #[test]
    fn plain_cls_jump_is_chip8() {
        let r = analyse(&[0x00, 0xE0, 0x12, 0x00]);
        assert_eq!(r.extension, ExtensionLevel::Chip8);
    }

    #[test]
    fn hires_classifies_as_superchip() {
        // 00FF (hires) 1200 (jump self)
        let r = analyse(&[0x00, 0xFF, 0x12, 0x00]);
        assert_eq!(r.extension, ExtensionLevel::SuperChip);
    }

    #[test]
    fn scroll_up_classifies_as_xochip() {
        // 00D2 (scroll-up 2) 1200 (jump)
        let r = analyse(&[0x00, 0xD2, 0x12, 0x00]);
        assert_eq!(r.extension, ExtensionLevel::XoChip);
    }

    #[test]
    fn set_i_long_classifies_as_xochip() {
        // F000 ABCD 1200
        let r = analyse(&[0xF0, 0x00, 0xAB, 0xCD, 0x12, 0x04]);
        assert_eq!(r.extension, ExtensionLevel::XoChip);
    }

    #[test]
    fn extension_level_ordering() {
        // Chip8 < SuperChip < XoChip
        assert!(ExtensionLevel::Chip8 < ExtensionLevel::SuperChip);
        assert!(ExtensionLevel::SuperChip < ExtensionLevel::XoChip);
    }

    // -----------------------------------------------------------------------
    // Pass C — Quirk Sensitivity
    // -----------------------------------------------------------------------

    #[test]
    fn shift_x_ne_y_emits_w002() {
        // 8106: Shr V1, V0  (x=1, y=0 — VIP style, differs from SCHIP)
        // Followed by 1200 jump to avoid falling off ROM
        let r = analyse(&[0x81, 0x06, 0x12, 0x00]);
        assert!(codes(&r).contains(&"W002"), "{:?}", r.diags);
    }

    #[test]
    fn shift_x_eq_y_no_w002() {
        // 8116: Shr V1, V1  (SCHIP style, no quirk warning)
        let r = analyse(&[0x81, 0x16, 0x12, 0x00]);
        assert!(!codes(&r).contains(&"W002"), "{:?}", r.diags);
    }

    #[test]
    fn save_regs_no_set_i_after_emits_w003() {
        // F255 (save v0..v2) 1200 (jump) — no SetI follows → W003
        let r = analyse(&[0xF2, 0x55, 0x12, 0x00]);
        assert!(codes(&r).contains(&"W003"), "{:?}", r.diags);
    }

    #[test]
    fn save_regs_followed_by_set_i_no_w003() {
        // F255 (save) A300 (SetI) 1200 (jump)
        let r = analyse(&[0xF2, 0x55, 0xA3, 0x00, 0x12, 0x04]);
        assert!(!codes(&r).contains(&"W003"), "{:?}", r.diags);
    }

    #[test]
    fn jump_offset_emits_w004() {
        // B300 — JumpOffset (W004 from analysis pass C, W005 from CFG)
        let r = analyse(&[0xB3, 0x00]);
        assert!(codes(&r).contains(&"W004"), "{:?}", r.diags);
    }

    #[test]
    fn or_followed_by_vf_read_emits_w006() {
        // 81F1: V1 |= VF  (OR, clobbers VF on VIP)
        // 3F00: skip if VF == 0  (reads VF)
        // 1200: jump
        let r = analyse(&[0x81, 0xF1, 0x3F, 0x00, 0x12, 0x04]);
        assert!(codes(&r).contains(&"W006"), "{:?}", r.diags);
    }

    // -----------------------------------------------------------------------
    // Pass D — Register Hazards
    // -----------------------------------------------------------------------

    #[test]
    fn explicit_set_vf_emits_w008() {
        // 6F05: SetImm VF := 5  — intentional flag register write
        let r = analyse(&[0x6F, 0x05, 0x12, 0x00]);
        assert!(codes(&r).contains(&"W008"), "{:?}", r.diags);
    }

    #[test]
    fn set_other_reg_no_w008() {
        // 6105: SetImm V1 := 5 — not VF
        let r = analyse(&[0x61, 0x05, 0x12, 0x00]);
        assert!(!codes(&r).contains(&"W008"), "{:?}", r.diags);
    }

    #[test]
    fn mov_to_vf_emits_w008() {
        // 8F10: VF := V1
        let r = analyse(&[0x8F, 0x10, 0x12, 0x00]);
        assert!(codes(&r).contains(&"W008"), "{:?}", r.diags);
    }
}
