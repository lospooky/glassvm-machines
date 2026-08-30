//! Hexwell catalyst decoding and toroidal lattice geometry.

use serde::{Deserialize, Serialize};

pub const GRID_WIDTH: usize = 16;
pub const GRID_HEIGHT: usize = 16;
pub const WELL_COUNT: usize = GRID_WIDTH * GRID_HEIGHT;
pub const CENTER_X: usize = 8;
pub const CENTER_Y: usize = 8;
pub const CENTER_WELL: usize = CENTER_Y * GRID_WIDTH + CENTER_X;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Materia {
    Brine,
    Ember,
}

impl Materia {
    pub fn from_polarity(polarity: bool) -> Self {
        if polarity { Self::Ember } else { Self::Brine }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Brine => "BRINE",
            Self::Ember => "EMBER",
        }
    }

    pub fn state_name(self) -> &'static str {
        match self {
            Self::Brine => "brine",
            Self::Ember => "ember",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Brine => Self::Ember,
            Self::Ember => Self::Brine,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Family {
    Dormant = 0x0,
    Drip = 0x1,
    Pour = 0x2,
    Osmose = 0x3,
    Precipitate = 0x4,
    Dissolve = 0x5,
    Bind = 0x6,
    Cleave = 0x7,
    Tincture = 0x8,
    Temper = 0x9,
    Kindle = 0xA,
    Quench = 0xB,
    Affinity = 0xC,
    Fork = 0xD,
    Seek = 0xE,
    Vent = 0xF,
}

impl Family {
    pub fn decode(op: u8) -> Self {
        match op & 0x0f {
            0x0 => Self::Dormant,
            0x1 => Self::Drip,
            0x2 => Self::Pour,
            0x3 => Self::Osmose,
            0x4 => Self::Precipitate,
            0x5 => Self::Dissolve,
            0x6 => Self::Bind,
            0x7 => Self::Cleave,
            0x8 => Self::Tincture,
            0x9 => Self::Temper,
            0xA => Self::Kindle,
            0xB => Self::Quench,
            0xC => Self::Affinity,
            0xD => Self::Fork,
            0xE => Self::Seek,
            0xF => Self::Vent,
            _ => unreachable!("nibble is masked"),
        }
    }

    pub fn mnemonic(self) -> &'static str {
        match self {
            Self::Dormant => "DORMANT",
            Self::Drip => "DRIP",
            Self::Pour => "POUR",
            Self::Osmose => "OSMOSE",
            Self::Precipitate => "PRECIP",
            Self::Dissolve => "DISSOLVE",
            Self::Bind => "BIND",
            Self::Cleave => "CLEAVE",
            Self::Tincture => "TINCTURE",
            Self::Temper => "TEMPER",
            Self::Kindle => "KINDLE",
            Self::Quench => "QUENCH",
            Self::Affinity => "AFFINITY",
            Self::Fork => "FORK",
            Self::Seek => "SEEK",
            Self::Vent => "VENT",
        }
    }
}

/// One immutable catalyst byte in the 16×16 Hexwell plate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalyst {
    pub byte: u8,
    pub family: Family,
    pub arg: u8,
}

impl Catalyst {
    pub fn decode(byte: u8) -> Self {
        Self {
            byte,
            family: Family::decode(byte >> 4),
            arg: byte & 0x0f,
        }
    }

    pub fn polarity(self) -> bool {
        self.arg & 0x08 != 0
    }

    pub fn materia(self) -> Materia {
        Materia::from_polarity(self.polarity())
    }

    pub fn selector(self) -> u8 {
        self.arg & 0x07
    }

    pub fn disassemble(self) -> String {
        let materia = self.materia().name();
        let direction = selector_name(self.selector());
        match self.family {
            Family::Dormant if self.polarity() => "HOLD".into(),
            Family::Dormant => "DARK".into(),
            Family::Drip => format!("DRIP {materia}, {direction}"),
            Family::Pour => format!("POUR {materia}, {direction}"),
            Family::Osmose => format!("OSMOSE {materia}, {direction}"),
            Family::Precipitate => format!("PRECIP {materia}, {direction}"),
            Family::Dissolve => format!("DISSOLVE {materia}, {direction}"),
            Family::Bind => format!("BIND {materia}, {direction}"),
            Family::Cleave => format!("CLEAVE {materia}, {direction}"),
            Family::Tincture => format!("TINCTURE {materia}, {direction}"),
            Family::Temper => format!("TEMPER {materia}, {direction}"),
            Family::Kindle => format!(
                "KINDLE {}, {direction}",
                if self.polarity() { "FIERCE" } else { "GENTLE" }
            ),
            Family::Quench => format!(
                "QUENCH {}, {direction}",
                if self.polarity() { "FIERCE" } else { "GENTLE" }
            ),
            Family::Affinity => format!("AFFINITY {materia}, {direction}"),
            Family::Fork => format!("FORK {materia}, {direction}"),
            Family::Seek => format!("SEEK {materia}, {}", self.selector()),
            Family::Vent => format!("VENT {materia}, {direction}"),
        }
    }
}

pub fn selector_name(selector: u8) -> &'static str {
    match selector & 0x07 {
        0 => "E",
        1 => "SE",
        2 => "SW",
        3 => "W",
        4 => "NW",
        5 => "NE",
        6 => "SELF",
        7 => "DOWN",
        _ => unreachable!("selector is masked"),
    }
}

pub fn well_index(x: usize, y: usize) -> usize {
    (y % GRID_HEIGHT) * GRID_WIDTH + (x % GRID_WIDTH)
}

pub fn well_coordinates(index: usize) -> (usize, usize) {
    (index % GRID_WIDTH, (index / GRID_WIDTH) % GRID_HEIGHT)
}

/// Neighbor order is E, SE, SW, W, NW, NE on an odd-row offset hex torus.
pub fn neighbor(index: usize, direction: u8) -> usize {
    let (x, y) = well_coordinates(index);
    let odd = y & 1;
    let (dx, dy) = match direction % 6 {
        0 => (1_i32, 0_i32),
        1 => (odd as i32, 1),
        2 => (odd as i32 - 1, 1),
        3 => (-1, 0),
        4 => (odd as i32 - 1, -1),
        5 => (odd as i32, -1),
        _ => unreachable!("direction is reduced modulo six"),
    };
    let nx = (x as i32 + dx).rem_euclid(GRID_WIDTH as i32) as usize;
    let ny = (y as i32 + dy).rem_euclid(GRID_HEIGHT as i32) as usize;
    well_index(nx, ny)
}

pub fn opposite(direction: u8) -> u8 {
    (direction + 3) % 6
}

pub fn portal(direction: u8) -> usize {
    let mut index = CENTER_WELL;
    for _ in 0..7 {
        index = neighbor(index, direction);
    }
    index
}
