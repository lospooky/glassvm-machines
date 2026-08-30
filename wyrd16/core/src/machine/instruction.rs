//! Fixed-width Wyrd-16 rune decoding and disassembly.

/// Decoded fixed-width instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rune {
    pub word: u16,
    pub op: u8,
    pub a: u8,
    pub b: u8,
    pub c: u8,
}

impl Rune {
    pub fn decode(high: u8, low: u8) -> Self {
        let word = u16::from_be_bytes([high, low]);
        Self {
            word,
            op: high >> 4,
            a: high & 0x0f,
            b: low >> 4,
            c: low & 0x0f,
        }
    }

    pub fn imm8(self) -> u8 {
        (self.b << 4) | self.c
    }

    pub fn addr12(self) -> u16 {
        self.word & 0x0fff
    }

    pub fn disassemble(self) -> String {
        match self.op {
            0x0 => match self.addr12() {
                0 => "NOP".into(),
                1 => "HALT".into(),
                2 => "CLEAR".into(),
                3 => "HOME".into(),
                n => format!("CHARM 0x{n:03X}"),
            },
            0x1 => format!("SET R{}, 0x{:02X}", self.a, self.imm8()),
            0x2 => format!("ADD R{}, 0x{:02X}", self.a, self.imm8()),
            0x3 => format!("XOR R{}, 0x{:02X}", self.a, self.imm8()),
            0x4 => format!("MOV R{}, R{}", self.a, self.b),
            0x5 => format!("LOAD R{}, [R{}:{}]", self.a, self.b, self.c),
            0x6 => format!("STORE R{}, [R{}:{}]", self.a, self.b, self.c),
            0x7 => format!("JUMP 0x{:03X}", self.addr12() & 0x0ffe),
            0x8 => format!("JNZ R{}, {:+}", self.a, self.imm8() as i8),
            0x9 => format!("RND R{}, 0x{:02X}", self.a, self.imm8()),
            0xA => format!("KEY R{}, 0x{:02X}", self.a, self.imm8()),
            0xB => format!("PEN R{}, R{}, {}", self.a, self.b, self.c),
            0xC => format!("PIX R{}, R{}, {}", self.a, self.b, self.c),
            0xD => format!("LINE R{}, R{}, {}", self.a, self.b, self.c),
            0xE => format!("PAL R{}, R{}, {}", self.a, self.b, self.c),
            0xF => format!("WEAVE R{}, R{}, 0b{:04b}", self.a, self.b, self.c),
            _ => unreachable!(),
        }
    }
}
