//! Complete serializable native machine state.

use serde::{Deserialize, Serialize};

use crate::{
    Artifact, CoreError, DISPLAY_HEIGHT, DISPLAY_PIXELS, DISPLAY_WIDTH, MEMORY_BYTES, Rune,
};

/// All state required to continue native Wyrd-16 execution exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineState {
    pub memory: Vec<u8>,
    pub registers: [u8; 16],
    pub pc: u16,
    pub cursor_x: u8,
    pub cursor_y: u8,
    pub ink: u8,
    pub palette: [u8; 16],
    pub canvas: Vec<u8>,
    pub input_mask: u8,
    pub rng: u64,
    pub halted: bool,
    pub cycles: u64,
    pub frames: u64,
}

impl MachineState {
    pub fn boot(bytes: &[u8], seed: u64) -> Result<Self, CoreError> {
        let artifact = Artifact::parse(bytes)?;
        let mut memory = vec![0; MEMORY_BYTES];
        memory[..artifact.bytes().len()].copy_from_slice(artifact.bytes());
        let palette = [
            0x00, 0xff, 0xe0, 0x1c, 0x03, 0xfc, 0xe3, 0x1f, 0x92, 0x49, 0xb6, 0x6d, 0xdb, 0x24,
            0x7f, 0xaa,
        ];
        Ok(Self {
            memory,
            registers: [0; 16],
            pc: 0,
            cursor_x: 32,
            cursor_y: 32,
            ink: 1,
            palette,
            canvas: vec![0; DISPLAY_PIXELS],
            input_mask: 0,
            rng: if seed == 0 {
                0x9e37_79b9_7f4a_7c15
            } else {
                seed
            },
            halted: false,
            cycles: 0,
            frames: 0,
        })
    }

    pub fn fetch(&self) -> Rune {
        let pc = self.pc as usize & 0x0ffe;
        Rune::decode(self.memory[pc], self.memory[pc + 1])
    }

    pub fn random_byte(&mut self) -> u8 {
        let mut value = self.rng;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.rng = value;
        (value >> 56) as u8
    }

    pub(crate) fn plot(&mut self, x: i16, y: i16, color: u8) -> bool {
        let x = x.rem_euclid(DISPLAY_WIDTH as i16) as usize;
        let y = y.rem_euclid(DISPLAY_HEIGHT as i16) as usize;
        let index = y * DISPLAY_WIDTH + x;
        let color = color & 0x0f;
        let changed = self.canvas[index] != color;
        self.canvas[index] = color;
        changed
    }

    pub(crate) fn line_to(&mut self, target_x: u8, target_y: u8, color: u8) -> usize {
        let (mut x0, mut y0) = (self.cursor_x as i16, self.cursor_y as i16);
        let (x1, y1) = ((target_x & 63) as i16, (target_y & 63) as i16);
        let delta_x = (x1 - x0).abs();
        let step_x = if x0 < x1 { 1 } else { -1 };
        let delta_y = -(y1 - y0).abs();
        let step_y = if y0 < y1 { 1 } else { -1 };
        let mut error = delta_x + delta_y;
        let mut changed = 0;
        loop {
            changed += usize::from(self.plot(x0, y0, color));
            if x0 == x1 && y0 == y1 {
                break;
            }
            let doubled_error = 2 * error;
            if doubled_error >= delta_y {
                error += delta_y;
                x0 += step_x;
            }
            if doubled_error <= delta_x {
                error += delta_x;
                y0 += step_y;
            }
        }
        self.cursor_x = target_x & 63;
        self.cursor_y = target_y & 63;
        changed
    }
}
