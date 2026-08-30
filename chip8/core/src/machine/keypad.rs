/// 16-key hexadecimal keypad state.
///
/// Keys are indexed 0x0–0xF.  `true` = currently pressed.
#[derive(Default, Clone, Copy)]
pub struct Keypad {
    state: [bool; 16],
}

impl Keypad {
    pub fn press(&mut self, key: u8) {
        if key < 16 {
            self.state[key as usize] = true;
        }
    }

    pub fn release(&mut self, key: u8) {
        if key < 16 {
            self.state[key as usize] = false;
        }
    }

    pub fn is_pressed(&self, key: u8) -> bool {
        key < 16 && self.state[key as usize]
    }

    /// Update from a full 16-element slice (true = pressed).
    pub fn set_all(&mut self, keys: &[bool; 16]) {
        self.state = *keys;
    }

    /// Return the index of the first pressed key, or `None`.
    pub fn any_pressed(&self) -> Option<u8> {
        self.state.iter().position(|&k| k).map(|i| i as u8)
    }

    /// Pack the 16-key state into a bitmask (bit N = key N pressed).
    pub fn as_mask(&self) -> u16 {
        let mut mask = 0u16;
        for i in 0..16 {
            if self.state[i] {
                mask |= 1 << i;
            }
        }
        mask
    }

    /// Set all key states from a bitmask (bit N = key N pressed).
    pub fn from_mask(&mut self, mask: u16) {
        for i in 0..16 {
            self.state[i] = (mask >> i) & 1 == 1;
        }
    }
}
