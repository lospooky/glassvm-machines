/// Decode and execute a single 16-bit instruction word against `cpu`.
///
/// Covers: CHIP-8 (all 35), SUPER-CHIP, and XO-CHIP opcodes.
use crate::cpu::{Cpu, KeyWait, StepResult};
use crate::event::{Event, EventLog, Timer};
use crate::font::{BIG_FONT_ADDR, BIG_FONT_SPRITE_SIZE, FONT_ADDR, FONT_SPRITE_SIZE};

pub fn execute(cpu: &mut Cpu, word: u16, events: &mut EventLog) -> StepResult {
    let top = ((word >> 12) & 0xF) as u8;
    let x = ((word >> 8) & 0xF) as usize;
    let y = ((word >> 4) & 0xF) as usize;
    let n = (word & 0xF) as u8;
    let nn = (word & 0xFF) as u8;
    let nnn = word & 0xFFF;

    match (top, x, y, n) {
        // --- 0 group ---

        // 00E0 — CLS
        (0x0, 0x0, 0xE, 0x0) => {
            cpu.display.clear();
            events.push(Event::ClearScreen);
            StepResult::Ok
        }

        // 00EE — RET
        (0x0, 0x0, 0xE, 0xE) => match cpu.pop() {
            None => StepResult::StackUnderflow,
            Some(ret) => {
                cpu.pc = ret;
                events.push(Event::Return { to: ret });
                events.push(Event::StackPop);
                StepResult::Ok
            }
        },

        // 00FD — EXIT (SUPER-CHIP)
        (0x0, 0x0, 0xF, 0xD) => {
            cpu.halted = true;
            StepResult::Halted
        }

        // 00FE — lores (SUPER-CHIP)
        (0x0, 0x0, 0xF, 0xE) => {
            cpu.display.hires = false;
            cpu.display.clear_all();
            events.push(Event::LoresEnabled);
            StepResult::Ok
        }

        // 00FF — hires (SUPER-CHIP)
        (0x0, 0x0, 0xF, 0xF) => {
            cpu.display.hires = true;
            cpu.display.clear_all();
            events.push(Event::HiresEnabled);
            StepResult::Ok
        }

        // 00FB — scroll-right 4 (SUPER-CHIP)
        (0x0, 0x0, 0xF, 0xB) => {
            cpu.display.scroll_right();
            events.push(Event::ScrollRight);
            StepResult::Ok
        }

        // 00FC — scroll-left 4 (SUPER-CHIP)
        (0x0, 0x0, 0xF, 0xC) => {
            cpu.display.scroll_left();
            events.push(Event::ScrollLeft);
            StepResult::Ok
        }

        // 00CN — scroll-down N (SUPER-CHIP)
        (0x0, 0x0, 0xC, _) => {
            cpu.display.scroll_down(n as usize);
            events.push(Event::ScrollDown { n });
            StepResult::Ok
        }

        // 00DN — scroll-up N (XO-CHIP)
        (0x0, 0x0, 0xD, _) => {
            cpu.display.scroll_up(n as usize);
            events.push(Event::ScrollUp { n });
            StepResult::Ok
        }

        // 0NNN — machine code call; silently treated as NOP on modern targets
        (0x0, _, _, _) => StepResult::Ok,

        // --- 1NNN — JP addr ---
        (0x1, _, _, _) => {
            let from = cpu.pc.wrapping_sub(2);
            cpu.pc = nnn;
            events.push(Event::Jump { from, to: nnn });
            StepResult::Ok
        }

        // --- 2NNN — CALL addr ---
        (0x2, _, _, _) => {
            let from = cpu.pc.wrapping_sub(2);
            let ret = cpu.pc;
            if !cpu.push(ret) {
                return StepResult::StackOverflow;
            }
            cpu.pc = nnn;
            events.push(Event::Call { from, to: nnn });
            events.push(Event::StackPush);
            StepResult::Ok
        }

        // --- 3XNN — SE Vx, byte ---
        (0x3, _, _, _) => {
            if cpu.v[x] == nn {
                cpu.pc = cpu.pc.wrapping_add(2);
            }
            StepResult::Ok
        }

        // --- 4XNN — SNE Vx, byte ---
        (0x4, _, _, _) => {
            if cpu.v[x] != nn {
                cpu.pc = cpu.pc.wrapping_add(2);
            }
            StepResult::Ok
        }

        // --- 5XY0 — SE Vx, Vy ---
        (0x5, _, _, 0x0) => {
            if cpu.v[x] == cpu.v[y] {
                cpu.pc = cpu.pc.wrapping_add(2);
            }
            StepResult::Ok
        }

        // --- 5XY2 — save vx–vy (XO-CHIP) ---
        (0x5, _, _, 0x2) => {
            let addr = cpu.i;
            let len = x.abs_diff(y) + 1;
            let end = addr as usize + len - 1;
            if end >= crate::cpu::MEM_SIZE {
                return StepResult::MemoryFault(crate::cpu::MEM_SIZE as u32);
            }
            let start = addr as usize;
            let before = events.capture_memory_before(&cpu.mem[start..=end]);
            for offset in 0..len {
                let reg = if x <= y { x + offset } else { x - offset };
                cpu.mem[cpu.i as usize + offset] = cpu.v[reg];
            }
            events.push_memory_write(addr, before, &cpu.mem[start..=end]);
            StepResult::Ok
        }

        // --- 5XY3 — load vx–vy (XO-CHIP) ---
        (0x5, _, _, 0x3) => {
            let len = x.abs_diff(y) + 1;
            let end = cpu.i as usize + len - 1;
            if end >= crate::cpu::MEM_SIZE {
                return StepResult::MemoryFault(crate::cpu::MEM_SIZE as u32);
            }
            events.record_memory_read(cpu.i, &cpu.mem[cpu.i as usize..=end]);
            for offset in 0..len {
                let reg = if x <= y { x + offset } else { x - offset };
                cpu.v[reg] = cpu.mem[cpu.i as usize + offset];
            }
            StepResult::Ok
        }

        // --- 6XNN — LD Vx, byte ---
        (0x6, _, _, _) => {
            cpu.v[x] = nn;
            StepResult::Ok
        }

        // --- 7XNN — ADD Vx, byte (no carry) ---
        (0x7, _, _, _) => {
            cpu.v[x] = cpu.v[x].wrapping_add(nn);
            StepResult::Ok
        }

        // --- 8 group — arithmetic / logic ---

        // 8XY0 — LD Vx, Vy
        (0x8, _, _, 0x0) => {
            cpu.v[x] = cpu.v[y];
            StepResult::Ok
        }

        // 8XY1 — OR Vx, Vy
        (0x8, _, _, 0x1) => {
            cpu.v[x] |= cpu.v[y];
            if cpu.quirks.vf_reset_on_logic {
                cpu.v[0xF] = 0;
            }
            StepResult::Ok
        }

        // 8XY2 — AND Vx, Vy
        (0x8, _, _, 0x2) => {
            cpu.v[x] &= cpu.v[y];
            if cpu.quirks.vf_reset_on_logic {
                cpu.v[0xF] = 0;
            }
            StepResult::Ok
        }

        // 8XY3 — XOR Vx, Vy
        (0x8, _, _, 0x3) => {
            cpu.v[x] ^= cpu.v[y];
            if cpu.quirks.vf_reset_on_logic {
                cpu.v[0xF] = 0;
            }
            StepResult::Ok
        }

        // 8XY4 — ADD Vx, Vy (with carry)
        (0x8, _, _, 0x4) => {
            let (res, carry) = cpu.v[x].overflowing_add(cpu.v[y]);
            cpu.v[x] = res;
            cpu.v[0xF] = carry as u8;
            StepResult::Ok
        }

        // 8XY5 — SUB Vx, Vy  (VF = NOT borrow = Vx >= Vy)
        (0x8, _, _, 0x5) => {
            let (res, borrow) = cpu.v[x].overflowing_sub(cpu.v[y]);
            cpu.v[x] = res;
            cpu.v[0xF] = (!borrow) as u8;
            StepResult::Ok
        }

        // 8XY6 — SHR Vx {, Vy}
        (0x8, _, _, 0x6) => {
            let src = if cpu.quirks.shift_vx_only {
                cpu.v[x]
            } else {
                cpu.v[y]
            };
            cpu.v[0xF] = src & 0x1;
            cpu.v[x] = src >> 1;
            StepResult::Ok
        }

        // 8XY7 — SUBN Vx, Vy  (VF = NOT borrow = Vy >= Vx)
        (0x8, _, _, 0x7) => {
            let (res, borrow) = cpu.v[y].overflowing_sub(cpu.v[x]);
            cpu.v[x] = res;
            cpu.v[0xF] = (!borrow) as u8;
            StepResult::Ok
        }

        // 8XYE — SHL Vx {, Vy}
        (0x8, _, _, 0xE) => {
            let src = if cpu.quirks.shift_vx_only {
                cpu.v[x]
            } else {
                cpu.v[y]
            };
            cpu.v[0xF] = (src >> 7) & 0x1;
            cpu.v[x] = src << 1;
            StepResult::Ok
        }

        // --- 9XY0 — SNE Vx, Vy ---
        (0x9, _, _, 0x0) => {
            if cpu.v[x] != cpu.v[y] {
                cpu.pc = cpu.pc.wrapping_add(2);
            }
            StepResult::Ok
        }

        // --- ANNN — LD I, addr ---
        (0xA, _, _, _) => {
            cpu.i = nnn;
            StepResult::Ok
        }

        // --- BNNN — JP V0/VX, addr (quirk-sensitive) ---
        (0xB, _, _, _) => {
            let from = cpu.pc.wrapping_sub(2);
            let offset = if cpu.quirks.jump_offset_vx {
                cpu.v[x] as u16
            } else {
                cpu.v[0] as u16
            };
            let to = nnn.wrapping_add(offset);
            cpu.pc = to;
            events.push(Event::Jump { from, to });
            StepResult::Ok
        }

        // --- CXNN — RND Vx, byte ---
        (0xC, _, _, _) => {
            cpu.v[x] = cpu.rng.next_u8() & nn;
            StepResult::Ok
        }

        // --- DXYN — DRW Vx, Vy, n ---
        (0xD, _, _, _) => {
            if let Some(fault) = draw(cpu, x, y, n, events) {
                return fault;
            }
            StepResult::Ok
        }

        // --- E group — key skip ---

        // EX9E — SKP Vx
        (0xE, _, 0x9, 0xE) => {
            if cpu.keys.is_pressed(cpu.v[x]) {
                cpu.pc = cpu.pc.wrapping_add(2);
            }
            StepResult::Ok
        }

        // EXA1 — SKNP Vx
        (0xE, _, 0xA, 0x1) => {
            if !cpu.keys.is_pressed(cpu.v[x]) {
                cpu.pc = cpu.pc.wrapping_add(2);
            }
            StepResult::Ok
        }

        // --- F group ---

        // FX07 — LD Vx, DT
        (0xF, _, 0x0, 0x7) => {
            cpu.v[x] = cpu.dt;
            StepResult::Ok
        }

        // FX0A — LD Vx, K  (wait for key)
        (0xF, _, 0x0, 0xA) => {
            cpu.key_wait = KeyWait::WaitPress(x as u8);
            events.push(Event::KeyWaitEntered);
            StepResult::Ok
        }

        // FX15 — LD DT, Vx
        (0xF, _, 0x1, 0x5) => {
            cpu.dt = cpu.v[x];
            events.push(Event::TimerSet {
                timer: Timer::Delay,
                value: cpu.v[x],
            });
            StepResult::Ok
        }

        // FX18 — LD ST, Vx
        (0xF, _, 0x1, 0x8) => {
            cpu.st = cpu.v[x];
            events.push(Event::TimerSet {
                timer: Timer::Sound,
                value: cpu.v[x],
            });
            StepResult::Ok
        }

        // FX1E — ADD I, Vx
        (0xF, _, 0x1, 0xE) => {
            cpu.i = cpu.i.wrapping_add(cpu.v[x] as u16);
            StepResult::Ok
        }

        // FX29 — LD F, Vx  (point I at small font sprite for digit Vx & 0xF)
        (0xF, _, 0x2, 0x9) => {
            let digit = (cpu.v[x] & 0xF) as u16;
            cpu.i = FONT_ADDR + digit * FONT_SPRITE_SIZE;
            StepResult::Ok
        }

        // FX30 — LD HF, Vx  (big font, SUPER-CHIP; digits 0–9 only)
        (0xF, _, 0x3, 0x0) => {
            let digit = (cpu.v[x] % 10) as u16;
            cpu.i = BIG_FONT_ADDR + digit * BIG_FONT_SPRITE_SIZE;
            StepResult::Ok
        }

        // FX33 — LD B, Vx  (BCD)
        (0xF, _, 0x3, 0x3) => {
            let val = cpu.v[x];
            let addr = cpu.i;
            let end = addr as usize + 2;
            if end >= crate::cpu::MEM_SIZE {
                return StepResult::MemoryFault(crate::cpu::MEM_SIZE as u32);
            }
            let start = addr as usize;
            let before = events.capture_memory_before(&cpu.mem[start..=end]);
            cpu.mem[addr as usize] = val / 100;
            cpu.mem[addr as usize + 1] = (val / 10) % 10;
            cpu.mem[addr as usize + 2] = val % 10;
            events.push_memory_write(addr, before, &cpu.mem[start..=end]);
            StepResult::Ok
        }

        // FX3A — pitch := Vx (XO-CHIP)
        (0xF, _, 0x3, 0xA) => {
            cpu.audio_pitch = cpu.v[x];
            StepResult::Ok
        }

        // FX55 — LD [I], Vx
        (0xF, _, 0x5, 0x5) => {
            let addr = cpu.i;
            let end = addr as usize + x;
            if end >= crate::cpu::MEM_SIZE {
                return StepResult::MemoryFault(crate::cpu::MEM_SIZE as u32);
            }
            let start = addr as usize;
            let before = events.capture_memory_before(&cpu.mem[start..=end]);
            for reg in 0..=x {
                cpu.mem[cpu.i as usize + reg] = cpu.v[reg];
            }
            if !cpu.quirks.load_store_no_inc_i {
                cpu.i = cpu.i.wrapping_add(x as u16 + 1);
            }
            events.push_memory_write(addr, before, &cpu.mem[start..=end]);
            StepResult::Ok
        }

        // FX65 — LD Vx, [I]
        (0xF, _, 0x6, 0x5) => {
            let end = cpu.i as usize + x;
            if end >= crate::cpu::MEM_SIZE {
                return StepResult::MemoryFault(crate::cpu::MEM_SIZE as u32);
            }
            events.record_memory_read(cpu.i, &cpu.mem[cpu.i as usize..=end]);
            for reg in 0..=x {
                cpu.v[reg] = cpu.mem[cpu.i as usize + reg];
            }
            if !cpu.quirks.load_store_no_inc_i {
                cpu.i = cpu.i.wrapping_add(x as u16 + 1);
            }
            StepResult::Ok
        }

        // FX75 — save flags V0–VX (SUPER-CHIP)
        (0xF, _, 0x7, 0x5) => {
            for reg in 0..=x.min(15) {
                cpu.flags[reg] = cpu.v[reg];
            }
            StepResult::Ok
        }

        // FX85 — load flags V0–VX (SUPER-CHIP)
        (0xF, _, 0x8, 0x5) => {
            for reg in 0..=x.min(15) {
                cpu.v[reg] = cpu.flags[reg];
            }
            StepResult::Ok
        }

        // F000 NNNN — LD I, long addr (XO-CHIP; word == 0xF000)
        (0xF, 0x0, 0x0, 0x0) => {
            let operand_pc = cpu.pc;
            let next_pc = operand_pc.wrapping_add(1);
            let hi_byte = cpu.mem[operand_pc as usize];
            let lo_byte = cpu.mem[next_pc as usize];
            if next_pc > operand_pc {
                let bytes = [hi_byte, lo_byte];
                events.record_memory_read(operand_pc, &bytes);
            } else {
                events.record_memory_read(operand_pc, std::slice::from_ref(&hi_byte));
                events.record_memory_read(next_pc, std::slice::from_ref(&lo_byte));
            }
            let hi = hi_byte as u16;
            let lo = lo_byte as u16;
            cpu.pc = cpu.pc.wrapping_add(2);
            cpu.i = (hi << 8) | lo;
            StepResult::Ok
        }

        // FN01 — plane n (XO-CHIP; lower nibble of x is plane mask)
        (0xF, _, 0x0, 0x1) => {
            cpu.display.plane = (x as u8) & 0x3;
            StepResult::Ok
        }

        // F002 — audio (XO-CHIP)
        (0xF, 0x0, 0x0, 0x2) => {
            let addr = cpu.i as usize;
            let Some(end) = addr.checked_add(16) else {
                return StepResult::MemoryFault(crate::cpu::MEM_SIZE as u32);
            };
            if end > crate::cpu::MEM_SIZE {
                return StepResult::MemoryFault(crate::cpu::MEM_SIZE as u32);
            }
            events.record_memory_read(cpu.i, &cpu.mem[addr..end]);
            cpu.audio_buf.copy_from_slice(&cpu.mem[addr..end]);
            StepResult::Ok
        }

        // Unknown / unimplemented opcode
        _ => {
            let pc = cpu.pc.wrapping_sub(2);
            events.push(Event::InvalidOpcode { pc, opcode: word });
            StepResult::InvalidOpcode(word)
        }
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Execute the draw instruction DXYN and emit a Draw event.
/// Returns `Some(MemoryFault)` if the sprite data would read past the end of memory.
fn draw(cpu: &mut Cpu, x: usize, y: usize, n: u8, events: &mut EventLog) -> Option<StepResult> {
    let raw_x = cpu.v[x];
    let raw_y = cpu.v[y];
    let vx = cpu.v[x] as usize % cpu.display.width();
    let vy = cpu.v[y] as usize % cpu.display.height();
    let clipping = cpu.quirks.clipping;
    let mut collision = false;

    let is_16x16 = n == 0;

    // Bounds-check before any memory reads.
    let base = cpu.i as usize;
    let bytes_per_plane = if is_16x16 { 32usize } else { n as usize };
    let selected_planes = (cpu.display.plane & 0x3).count_ones() as usize;
    let byte_count = bytes_per_plane * selected_planes;
    if byte_count > 0 {
        let end = base + byte_count - 1;
        if end >= crate::cpu::MEM_SIZE {
            return Some(StepResult::MemoryFault(crate::cpu::MEM_SIZE as u32));
        }
    }

    let plane_mask = cpu.display.plane;
    if plane_mask != 0 && byte_count > 0 {
        events.record_memory_read(cpu.i, &cpu.mem[base..base + byte_count]);
    }
    let mut selected_index = 0usize;
    for p in 0..2u8 {
        if plane_mask & (1 << p) == 0 {
            continue;
        }
        let plane_base = base + selected_index * bytes_per_plane;
        selected_index += 1;

        let rows: u8 = if is_16x16 { 16 } else { n };
        for row in 0..rows as usize {
            if is_16x16 {
                let b0 = cpu.mem[plane_base + row * 2];
                let b1 = cpu.mem[plane_base + row * 2 + 1];
                let py = vy + row;
                if py >= cpu.display.height() && clipping {
                    continue;
                }
                if cpu
                    .display
                    .xor_sprite_byte(p as usize, vx, py, b0, 8, clipping)
                {
                    collision = true;
                }
                if cpu
                    .display
                    .xor_sprite_byte(p as usize, vx + 8, py, b1, 8, clipping)
                {
                    collision = true;
                }
            } else {
                let byte = cpu.mem[plane_base + row];
                let py = vy + row;
                if cpu
                    .display
                    .xor_sprite_byte(p as usize, vx, py, byte, 8, clipping)
                {
                    collision = true;
                }
            }
        }
    }

    cpu.v[0xF] = collision as u8;
    events.push(Event::Draw {
        x: raw_x,
        y: raw_y,
        n,
        collision,
    });
    None
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::QuirksConfig;
    use crate::cpu::{Cpu, KeyWait, ROM_START, StepResult};
    use crate::event::EventLog;

    fn cpu() -> Cpu {
        let mut c = Cpu::new(QuirksConfig::default());
        c.pc = ROM_START;
        c
    }

    fn step_word(cpu: &mut Cpu, word: u16) -> StepResult {
        let mut events = EventLog::new();
        execute(cpu, word, &mut events)
    }

    // ── 00E0 CLS ──
    #[test]
    fn cls_clears_display() {
        let mut c = cpu();
        c.display.buf[0][0] = 1;
        step_word(&mut c, 0x00E0);
        assert_eq!(c.display.buf[0][0], 0);
    }

    // ── 00EE RET / 2NNN CALL ──
    #[test]
    fn call_and_ret() {
        let mut c = cpu();
        // Simulate: fetch already advanced PC past the CALL opcode
        c.pc = 0x202;
        step_word(&mut c, 0x2300); // CALL 0x300 — ret addr = 0x202
        assert_eq!(c.pc, 0x300);
        assert_eq!(c.sp, 1);
        step_word(&mut c, 0x00EE); // RET → back to 0x202
        assert_eq!(c.pc, 0x202);
        assert_eq!(c.sp, 0);
    }

    // ── 1NNN JP ──
    #[test]
    fn jump() {
        let mut c = cpu();
        step_word(&mut c, 0x1ABC);
        assert_eq!(c.pc, 0xABC);
    }

    // ── 3XNN SE Vx,byte — skip ──
    #[test]
    fn se_byte_skip() {
        let mut c = cpu();
        c.v[2] = 0x42;
        c.pc = 0x200;
        step_word(&mut c, 0x3242); // SE V2, 0x42 — match, skip
        assert_eq!(c.pc, 0x202);
    }

    // ── 3XNN SE Vx,byte — no skip ──
    #[test]
    fn se_byte_no_skip() {
        let mut c = cpu();
        c.v[2] = 0x00;
        c.pc = 0x200;
        step_word(&mut c, 0x3242); // no match
        assert_eq!(c.pc, 0x200);
    }

    // ── 4XNN SNE ──
    #[test]
    fn sne_byte_skip() {
        let mut c = cpu();
        c.v[1] = 0x10;
        c.pc = 0x200;
        step_word(&mut c, 0x4111); // SNE V1, 0x11 — not equal, skip
        assert_eq!(c.pc, 0x202);
    }

    // ── 5XY0 SE Vx,Vy ──
    #[test]
    fn se_reg_skip() {
        let mut c = cpu();
        c.v[0] = 7;
        c.v[1] = 7;
        c.pc = 0x200;
        step_word(&mut c, 0x5010);
        assert_eq!(c.pc, 0x202);
    }

    // ── 6XNN LD ──
    #[test]
    fn ld_vx_byte() {
        let mut c = cpu();
        step_word(&mut c, 0x6A5F); // V[A] = 0x5F
        assert_eq!(c.v[0xA], 0x5F);
    }

    // ── 7XNN ADD no carry ──
    #[test]
    fn add_byte_no_carry() {
        let mut c = cpu();
        c.v[0] = 0xFE;
        step_word(&mut c, 0x7002); // V0 += 2 (wraps to 0)
        assert_eq!(c.v[0], 0x00);
        assert_eq!(c.v[0xF], 0); // VF untouched
    }

    // ── 8XY0 LD Vx,Vy ──
    #[test]
    fn ld_vx_vy() {
        let mut c = cpu();
        c.v[3] = 0xAB;
        step_word(&mut c, 0x8230); // V2 = V3
        assert_eq!(c.v[2], 0xAB);
    }

    // ── 8XY1 OR ──
    #[test]
    fn or_regs() {
        let mut c = cpu();
        c.v[0] = 0xF0;
        c.v[1] = 0x0F;
        step_word(&mut c, 0x8011);
        assert_eq!(c.v[0], 0xFF);
    }

    // ── 8XY2 AND ──
    #[test]
    fn and_regs() {
        let mut c = cpu();
        c.v[0] = 0xFF;
        c.v[1] = 0x0F;
        step_word(&mut c, 0x8012);
        assert_eq!(c.v[0], 0x0F);
    }

    // ── 8XY3 XOR ──
    #[test]
    fn xor_regs() {
        let mut c = cpu();
        c.v[0] = 0xFF;
        c.v[1] = 0x0F;
        step_word(&mut c, 0x8013);
        assert_eq!(c.v[0], 0xF0);
    }

    // ── 8XY4 ADD with carry ──
    #[test]
    fn add_carry() {
        let mut c = cpu();
        c.v[0] = 0xFF;
        c.v[1] = 0x01;
        step_word(&mut c, 0x8014);
        assert_eq!(c.v[0], 0x00);
        assert_eq!(c.v[0xF], 1); // carry
    }

    #[test]
    fn add_no_carry() {
        let mut c = cpu();
        c.v[0] = 0x01;
        c.v[1] = 0x01;
        step_word(&mut c, 0x8014);
        assert_eq!(c.v[0], 0x02);
        assert_eq!(c.v[0xF], 0);
    }

    // ── 8XY5 SUB ──
    #[test]
    fn sub_no_borrow() {
        let mut c = cpu();
        c.v[0] = 0x05;
        c.v[1] = 0x03;
        step_word(&mut c, 0x8015); // V0 -= V1
        assert_eq!(c.v[0], 0x02);
        assert_eq!(c.v[0xF], 1); // no borrow
    }

    #[test]
    fn sub_borrow() {
        let mut c = cpu();
        c.v[0] = 0x01;
        c.v[1] = 0x05;
        step_word(&mut c, 0x8015);
        assert_eq!(c.v[0], 0xFC);
        assert_eq!(c.v[0xF], 0); // borrow
    }

    // ── 8XY6 SHR (VIP mode: source = Vy) ──
    #[test]
    fn shr_vy_source() {
        let mut c = cpu(); // shift_vx_only = false
        c.v[1] = 0b0000_0110; // V1 = 6
        step_word(&mut c, 0x8016); // V0 = V1 >> 1
        assert_eq!(c.v[0], 3);
        assert_eq!(c.v[0xF], 0); // LSB of source was 0
    }

    #[test]
    fn shr_carries_out_bit() {
        let mut c = cpu();
        c.v[1] = 0b0000_0101;
        step_word(&mut c, 0x8016);
        assert_eq!(c.v[0], 0b0000_0010);
        assert_eq!(c.v[0xF], 1);
    }

    // ── 8XY6 SHR (SCHIP mode: source = Vx) ──
    #[test]
    fn shr_schip_mode() {
        let mut c = Cpu::new(QuirksConfig::schip());
        c.v[0] = 0b0000_0110;
        c.v[1] = 0b1111_1111;
        step_word(&mut c, 0x8016); // V0 = V0 >> 1 (ignores V1)
        assert_eq!(c.v[0], 3);
    }

    // ── 8XY7 SUBN ──
    #[test]
    fn subn_no_borrow() {
        let mut c = cpu();
        c.v[0] = 0x03;
        c.v[1] = 0x05;
        step_word(&mut c, 0x8017); // V0 = V1 - V0 = 2
        assert_eq!(c.v[0], 0x02);
        assert_eq!(c.v[0xF], 1);
    }

    // ── 8XYE SHL ──
    #[test]
    fn shl_vy_source() {
        let mut c = cpu();
        c.v[1] = 0b1100_0000;
        step_word(&mut c, 0x801E); // V0 = V1 << 1
        assert_eq!(c.v[0], 0b1000_0000);
        assert_eq!(c.v[0xF], 1); // MSB was 1
    }

    // ── 9XY0 SNE Vx,Vy ──
    #[test]
    fn sne_regs_skip() {
        let mut c = cpu();
        c.v[0] = 1;
        c.v[1] = 2;
        c.pc = 0x200;
        step_word(&mut c, 0x9010);
        assert_eq!(c.pc, 0x202);
    }

    // ── ANNN LD I ──
    #[test]
    fn ld_i() {
        let mut c = cpu();
        step_word(&mut c, 0xA123);
        assert_eq!(c.i, 0x123);
    }

    // ── BNNN JP V0 (VIP) ──
    #[test]
    fn jp_v0() {
        let mut c = cpu();
        c.v[0] = 0x10;
        step_word(&mut c, 0xB200); // PC = V0 + 0x200 = 0x210
        assert_eq!(c.pc, 0x210);
    }

    // ── BNNN JP VX (SCHIP) ──
    #[test]
    fn jp_vx_schip() {
        let mut c = Cpu::new(QuirksConfig::schip());
        c.v[2] = 0x04;
        step_word(&mut c, 0xB200); // PC = V2 + 0x200 = 0x204
        assert_eq!(c.pc, 0x204);
    }

    // ── CXNN RND ──
    #[test]
    fn rnd_masked() {
        let mut c = cpu();
        step_word(&mut c, 0xC00F); // V0 = rng & 0x0F
        assert_eq!(c.v[0] & !0x0F, 0, "high nibble must be zero due to mask");
    }

    // ── FX07 LD Vx, DT ──
    #[test]
    fn ld_vx_dt() {
        let mut c = cpu();
        c.dt = 42;
        step_word(&mut c, 0xF007);
        assert_eq!(c.v[0], 42);
    }

    // ── FX15 LD DT, Vx ──
    #[test]
    fn ld_dt_vx() {
        let mut c = cpu();
        c.v[0] = 10;
        step_word(&mut c, 0xF015);
        assert_eq!(c.dt, 10);
    }

    // ── FX18 LD ST, Vx ──
    #[test]
    fn ld_st_vx() {
        let mut c = cpu();
        c.v[1] = 5;
        step_word(&mut c, 0xF118);
        assert_eq!(c.st, 5);
    }

    // ── FX1E ADD I, Vx ──
    #[test]
    fn add_i_vx() {
        let mut c = cpu();
        c.i = 0x300;
        c.v[2] = 0x10;
        step_word(&mut c, 0xF21E);
        assert_eq!(c.i, 0x310);
    }

    // ── FX29 LD F, Vx ──
    #[test]
    fn ld_font() {
        let mut c = cpu();
        c.v[0] = 0x5;
        step_word(&mut c, 0xF029);
        assert_eq!(c.i, FONT_ADDR + 5 * FONT_SPRITE_SIZE);
    }

    // ── FX33 BCD ──
    #[test]
    fn bcd() {
        let mut c = cpu();
        c.v[0] = 254;
        c.i = 0x300;
        step_word(&mut c, 0xF033);
        assert_eq!(c.mem[0x300], 2);
        assert_eq!(c.mem[0x301], 5);
        assert_eq!(c.mem[0x302], 4);
    }

    // ── FX55 store memory (VIP: I increments) ──
    #[test]
    fn store_mem_inc_i() {
        let mut c = cpu();
        c.v[0] = 0xAA;
        c.v[1] = 0xBB;
        c.i = 0x300;
        step_word(&mut c, 0xF155); // store V0..V1
        assert_eq!(c.mem[0x300], 0xAA);
        assert_eq!(c.mem[0x301], 0xBB);
        assert_eq!(c.i, 0x302); // I incremented by X+1=2
    }

    // ── FX55 store memory (SCHIP: I unchanged) ──
    #[test]
    fn store_mem_no_inc_i() {
        let mut c = Cpu::new(QuirksConfig::schip());
        c.v[0] = 0x11;
        c.v[1] = 0x22;
        c.i = 0x400;
        step_word(&mut c, 0xF155);
        assert_eq!(c.i, 0x400); // I unchanged
    }

    // ── FX65 load memory ──
    #[test]
    fn load_mem() {
        let mut c = cpu();
        c.mem[0x300] = 0x55;
        c.mem[0x301] = 0x66;
        c.i = 0x300;
        step_word(&mut c, 0xF165); // load V0..V1
        assert_eq!(c.v[0], 0x55);
        assert_eq!(c.v[1], 0x66);
        assert_eq!(c.i, 0x302);
    }

    // ── FX75 / FX85 flag registers ──
    #[test]
    fn save_load_flags() {
        let mut c = cpu();
        c.v[0] = 0x12;
        c.v[1] = 0x34;
        step_word(&mut c, 0xF175); // save V0..V1 into flags
        c.v[0] = 0;
        c.v[1] = 0;
        step_word(&mut c, 0xF185); // load flags back into V0..V1
        assert_eq!(c.v[0], 0x12);
        assert_eq!(c.v[1], 0x34);
    }

    // ── tick_timers ──
    #[test]
    fn timers_decrement() {
        let mut c = cpu();
        c.dt = 2;
        c.st = 1;
        c.tick_timers();
        assert_eq!(c.dt, 1);
        assert_eq!(c.st, 0);
        c.tick_timers();
        assert_eq!(c.dt, 0);
        assert_eq!(c.st, 0); // doesn't underflow
    }

    // ── VF reset on logic (VIP quirk) ──
    #[test]
    fn vf_reset_on_or() {
        let mut c = Cpu::new(QuirksConfig::vip());
        c.v[0] = 0xF0;
        c.v[1] = 0x0F;
        c.v[0xF] = 0xFF; // set VF to something
        step_word(&mut c, 0x8011); // OR
        assert_eq!(c.v[0xF], 0, "VIP quirk: OR resets VF");
    }

    // ── DXYN collision flag ──
    #[test]
    fn draw_collision() {
        let mut c = cpu();
        // Write a sprite byte at I
        c.i = 0x300;
        c.mem[0x300] = 0xFF; // all 8 pixels on
        c.v[0] = 0;
        c.v[1] = 0;
        // Draw once — no collision
        let mut events = EventLog::new();
        draw(&mut c, 0, 1, 1, &mut events);
        assert_eq!(c.v[0xF], 0);
        // Draw again at same position — XOR erases → collision
        draw(&mut c, 0, 1, 1, &mut events);
        assert_eq!(c.v[0xF], 1);
    }

    #[test]
    fn draw_event_preserves_operand_coordinates_when_vf_is_an_operand() {
        let mut c = cpu();
        c.i = 0x300;
        c.mem[0x300] = 0x80;
        c.v[0xF] = 5;
        c.v[1] = 6;
        let mut events = EventLog::new();

        draw(&mut c, 0xF, 1, 1, &mut events);

        assert_eq!(c.v[0xF], 0, "VF is replaced by the collision flag");
        assert!(matches!(
            events.as_slice().last(),
            Some(Event::Draw { x: 5, y: 6, .. })
        ));
    }

    #[test]
    fn draw_uses_consecutive_sprite_bytes_for_each_selected_plane() {
        let mut c = cpu();
        c.display.plane = 0x3;
        c.i = 0x300;
        c.mem[0x300] = 0x80;
        c.mem[0x301] = 0x40;
        let mut events = EventLog::new();

        draw(&mut c, 0, 1, 1, &mut events);

        assert_eq!(c.display.get(0, 0, 0), 1);
        assert_eq!(c.display.get(0, 1, 0), 0);
        assert_eq!(c.display.get(1, 0, 0), 0);
        assert_eq!(c.display.get(1, 1, 0), 1);
    }

    #[test]
    fn dxy0_draws_a_real_16_by_16_sprite_in_lores() {
        let mut c = cpu();
        c.display.plane = 0x1;
        c.i = 0x300;
        c.mem[0x300] = 0x80;
        let mut events = EventLog::new();

        draw(&mut c, 0, 1, 0, &mut events);

        assert_eq!(c.display.get(0, 0, 0), 1);
    }

    // ── 5XY2 / 5XY3 XO-CHIP range I/O ──
    #[test]
    fn xochip_save_load_range() {
        let mut c = cpu();
        c.v[1] = 0xAA;
        c.v[2] = 0xBB;
        c.v[3] = 0xCC;
        c.i = 0x400;
        step_word(&mut c, 0x5132); // save V1–V3
        assert_eq!(c.mem[0x400], 0xAA);
        assert_eq!(c.mem[0x401], 0xBB);
        assert_eq!(c.mem[0x402], 0xCC);
        c.v[1] = 0;
        c.v[2] = 0;
        c.v[3] = 0;
        step_word(&mut c, 0x5133); // load V1–V3
        assert_eq!(c.v[1], 0xAA);
        assert_eq!(c.v[2], 0xBB);
        assert_eq!(c.v[3], 0xCC);
    }

    #[test]
    fn xochip_reverse_range_preserves_vx_to_vy_order() {
        let mut c = cpu();
        c.v[3] = 0xCC;
        c.v[2] = 0xBB;
        c.v[1] = 0xAA;
        c.i = 0x400;
        step_word(&mut c, 0x5312); // save V3–V1
        assert_eq!(&c.mem[0x400..0x403], &[0xCC, 0xBB, 0xAA]);

        c.v[1] = 0;
        c.v[2] = 0;
        c.v[3] = 0;
        step_word(&mut c, 0x5313); // load V3–V1
        assert_eq!([c.v[3], c.v[2], c.v[1]], [0xCC, 0xBB, 0xAA]);
    }

    #[test]
    fn xochip_audio_load_reports_a_memory_fault_instead_of_panicking() {
        let mut c = cpu();
        c.i = u16::MAX - 7;
        assert_eq!(
            step_word(&mut c, 0xF002),
            StepResult::MemoryFault(crate::cpu::MEM_SIZE as u32)
        );
    }

    // ── EX9E / EXA1 key skip ──
    #[test]
    fn skip_key_pressed() {
        let mut c = cpu();
        c.v[0] = 5;
        c.keys.press(5);
        c.pc = 0x200;
        step_word(&mut c, 0xE09E); // SKP V0
        assert_eq!(c.pc, 0x202);
    }

    #[test]
    fn skip_key_not_pressed() {
        let mut c = cpu();
        c.v[0] = 5;
        c.pc = 0x200;
        step_word(&mut c, 0xEAA1); // SKNP VA — key 5 not pressed — skip
        // Wait, this is SKNP for VA. The key isn't pressed so skip.
        // Actually opcode is EXA1, so x comes from (word>>8)&0xF = 0xA -> v[0xA]
        // Let me redo:
        c.v[0xA] = 7; // key 7 not pressed
        c.pc = 0x200;
        step_word(&mut c, 0xEAA1); // SKNP VA
        assert_eq!(c.pc, 0x202);
    }

    // ── FX0A wait for key ──
    #[test]
    fn wait_for_key() {
        let mut c = cpu();
        step_word(&mut c, 0xF00A); // wait for key into V0
        assert!(matches!(c.key_wait, KeyWait::WaitPress(0)));
        // tick with no key pressed — stays waiting
        let mut events = EventLog::new();
        assert!(matches!(c.tick(&mut events), StepResult::WaitingForKey));
        assert!(matches!(c.key_wait, KeyWait::WaitPress(0)));
        // press key 3
        c.keys.press(3);
        assert!(matches!(c.tick(&mut events), StepResult::WaitingForKey)); // transitions to WaitRelease
        assert!(matches!(c.key_wait, KeyWait::WaitRelease(0, 3)));
        // release key 3
        c.keys.release(3);
        assert!(matches!(c.tick(&mut events), StepResult::Ok)); // completes
        assert_eq!(c.v[0], 3);
        assert!(matches!(c.key_wait, KeyWait::None));
    }

    // ── 00FD halts CPU ──
    #[test]
    fn halt() {
        let mut c = cpu();
        step_word(&mut c, 0x00FD);
        assert!(c.halted);
        let mut events = EventLog::new();
        assert!(matches!(c.tick(&mut events), StepResult::Halted)); // halted stays halted
    }

    // ── Scroll instructions ──
    #[test]
    fn scroll_down_clears_top() {
        let mut c = cpu();
        c.display.plane = 0x1;
        // set pixel at (0,0)
        c.display.buf[0][0] = 1;
        let mut events = EventLog::new();
        execute(&mut c, 0x00C2, &mut events); // scroll-down 2
        // After scroll down 2 rows, row 0 should be clear in lores
        // In lores, pixel (0,0) maps to buf index 0; after scroll down 2
        // that pixel should now be at row 2 (index 2*2*128 = 512)
        assert_eq!(c.display.buf[0][0], 0, "top row should be cleared");
    }

    // ── hires / lores toggle ──
    #[test]
    fn hires_toggle() {
        let mut c = cpu();
        assert!(!c.display.hires);
        let mut events = EventLog::new();
        execute(&mut c, 0x00FF, &mut events);
        assert!(c.display.hires);
        execute(&mut c, 0x00FE, &mut events);
        assert!(!c.display.hires);
    }
}
