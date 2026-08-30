//! One-rune deterministic transition semantics.

use crate::{MEMORY_BYTES, MachineState, NativeEffectKind, StepEffect};

pub fn execute_step(state: &mut MachineState) -> StepEffect {
    let pc = state.pc & 0x0ffe;
    let rune = state.fetch();
    let mut next_pc = (pc + 2) & 0x0ffe;
    let mut effect = StepEffect {
        pc,
        next_pc,
        rune,
        kind: NativeEffectKind::InstructionDecoded,
        register_write: None,
        memory_access: None,
        display_writes: 0,
        palette_write: None,
        input_sample: None,
        random_sample: None,
        branch_taken: false,
    };

    match rune.op {
        0x0 => match rune.addr12() {
            1 => {
                state.halted = true;
                effect.kind = NativeEffectKind::RunHalted;
            }
            2 => {
                effect.display_writes = state.canvas.iter().filter(|pixel| **pixel != 0).count();
                state.canvas.fill(0);
                effect.kind = NativeEffectKind::DisplayWrite;
            }
            3 => {
                state.cursor_x = 32;
                state.cursor_y = 32;
                state.ink = 1;
            }
            _ => {}
        },
        0x1..=0x3 => {
            let old = state.registers[rune.a as usize];
            let new = match rune.op {
                0x1 => rune.imm8(),
                0x2 => old.wrapping_add(rune.imm8()),
                0x3 => old ^ rune.imm8(),
                _ => unreachable!(),
            };
            state.registers[rune.a as usize] = new;
            effect.register_write = Some((rune.a, old, new));
            effect.kind = NativeEffectKind::RegisterWrite;
        }
        0x4 => {
            let old = state.registers[rune.a as usize];
            let new = state.registers[rune.b as usize];
            state.registers[rune.a as usize] = new;
            effect.register_write = Some((rune.a, old, new));
            effect.kind = NativeEffectKind::RegisterWrite;
        }
        0x5 => {
            let address = (u16::from(state.registers[rune.b as usize]) << 4) | u16::from(rune.c);
            let old = state.registers[rune.a as usize];
            let new = state.memory[address as usize];
            state.registers[rune.a as usize] = new;
            effect.register_write = Some((rune.a, old, new));
            effect.memory_access = Some((false, address, new, new));
            effect.kind = NativeEffectKind::MemoryRead;
        }
        0x6 => {
            let address = (u16::from(state.registers[rune.b as usize]) << 4) | u16::from(rune.c);
            let old = state.memory[address as usize];
            let new = state.registers[rune.a as usize];
            state.memory[address as usize] = new;
            effect.memory_access = Some((true, address, old, new));
            effect.kind = NativeEffectKind::MemoryWrite;
        }
        0x7 => {
            next_pc = rune.addr12() & 0x0ffe;
            effect.kind = NativeEffectKind::BranchTaken;
            effect.branch_taken = true;
        }
        0x8 => {
            if state.registers[rune.a as usize] != 0 {
                let delta = (rune.imm8() as i8 as i16) * 2;
                next_pc =
                    ((next_pc as i16 + delta).rem_euclid(MEMORY_BYTES as i16) as u16) & 0x0ffe;
                effect.kind = NativeEffectKind::BranchTaken;
                effect.branch_taken = true;
            }
        }
        0x9 => {
            let sample = state.random_byte();
            let old = state.registers[rune.a as usize];
            let new = sample & rune.imm8();
            state.registers[rune.a as usize] = new;
            effect.register_write = Some((rune.a, old, new));
            effect.random_sample = Some(sample);
            effect.kind = NativeEffectKind::RegisterWrite;
        }
        0xA => {
            let sample = state.input_mask & rune.imm8();
            let old = state.registers[rune.a as usize];
            state.registers[rune.a as usize] = sample;
            effect.register_write = Some((rune.a, old, sample));
            effect.input_sample = Some(sample);
            effect.kind = NativeEffectKind::InputSampled;
        }
        0xB => {
            state.cursor_x = state.registers[rune.a as usize] & 63;
            state.cursor_y = state.registers[rune.b as usize] & 63;
            state.ink = rune.c;
        }
        0xC => {
            let x = state.registers[rune.a as usize];
            let y = state.registers[rune.b as usize];
            let color = if rune.c == 0 { state.ink } else { rune.c };
            effect.display_writes = usize::from(state.plot(x.into(), y.into(), color));
            effect.kind = NativeEffectKind::DisplayWrite;
        }
        0xD => {
            let x = state.registers[rune.a as usize];
            let y = state.registers[rune.b as usize];
            let color = if rune.c == 0 { state.ink } else { rune.c };
            effect.display_writes = state.line_to(x, y, color);
            effect.kind = NativeEffectKind::DisplayWrite;
        }
        0xE => {
            let index = rune.c as usize;
            let old = state.palette[index];
            let new = state.registers[rune.a as usize]
                .rotate_left((state.registers[rune.b as usize] & 7).into());
            state.palette[index] = new;
            effect.palette_write = Some((rune.c, old, new));
            effect.kind = NativeEffectKind::DisplayWrite;
        }
        0xF => {
            let x = (state.registers[rune.a as usize] & 63) as i16;
            let y = (state.registers[rune.b as usize] & 63) as i16;
            let color = state.ink;
            let mut points = vec![(x, y)];
            if rune.c & 1 != 0 {
                points.push((63 - x, y));
            }
            if rune.c & 2 != 0 {
                points.push((x, 63 - y));
            }
            if rune.c & 4 != 0 {
                points.push((63 - x, 63 - y));
            }
            if rune.c & 8 != 0 {
                points.extend([(y, x), (63 - y, x), (y, 63 - x)]);
            }
            points.sort_unstable();
            points.dedup();
            effect.display_writes = points
                .into_iter()
                .map(|(point_x, point_y)| usize::from(state.plot(point_x, point_y, color)))
                .sum();
            effect.kind = NativeEffectKind::DisplayWrite;
        }
        _ => unreachable!(),
    }
    state.pc = next_pc;
    state.cycles += 1;
    effect.next_pc = next_pc;
    effect
}
