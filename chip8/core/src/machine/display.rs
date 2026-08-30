/// CHIP-8 display subsystem.
///
/// Supports:
/// - 64×32 (lores) and 128×64 (hires / SUPER-CHIP) resolutions.
/// - Two independent bitplanes for XO-CHIP 4-colour rendering.
///
/// Pixels are stored as `u8` (0 or 1) in row-major order in a 128×64 backing
/// buffer regardless of the current resolution.  In lores mode each logical
/// pixel occupies a 2×2 block of the buffer.
pub const LORES_W: usize = 64;
pub const LORES_H: usize = 32;
pub const HIRES_W: usize = 128;
pub const HIRES_H: usize = 64;
pub const BUF_SIZE: usize = HIRES_W * HIRES_H;

#[derive(Clone)]
pub struct Display {
    /// Two bitplane buffers (XO-CHIP). plane[0] = classic single-colour.
    pub buf: [[u8; BUF_SIZE]; 2],
    /// True = 128×64 (SUPER-CHIP hires), false = 64×32.
    pub hires: bool,
    /// XO-CHIP plane bitmask (bits 0–1). 0x1 = plane 0, 0x2 = plane 1,
    /// 0x3 = both. Default 0x1 (classic single-plane).
    pub plane: u8,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            buf: [[0; BUF_SIZE]; 2],
            hires: false,
            plane: 0x1,
        }
    }
}

impl Display {
    pub fn clear(&mut self) {
        for p in 0..2 {
            if self.plane & (1 << p) != 0 {
                self.buf[p].fill(0);
            }
        }
    }

    pub fn clear_all(&mut self) {
        self.buf[0].fill(0);
        self.buf[1].fill(0);
    }

    #[inline]
    pub fn width(&self) -> usize {
        if self.hires { HIRES_W } else { LORES_W }
    }

    #[inline]
    pub fn height(&self) -> usize {
        if self.hires { HIRES_H } else { LORES_H }
    }

    #[inline]
    fn idx(&self, x: usize, y: usize) -> usize {
        if self.hires {
            y * HIRES_W + x
        } else {
            // lores: each logical pixel maps to (2x, 2y) in the 128×64 buffer
            (y * 2) * HIRES_W + (x * 2)
        }
    }

    #[inline]
    pub fn get(&self, plane_idx: usize, x: usize, y: usize) -> u8 {
        self.buf[plane_idx][self.idx(x, y)]
    }

    #[inline]
    fn set_pixel(&mut self, plane_idx: usize, x: usize, y: usize, val: u8) {
        let idx = self.idx(x, y);
        self.buf[plane_idx][idx] = val;
        if !self.hires {
            // fill the 2×2 logical pixel block
            self.buf[plane_idx][idx + 1] = val;
            self.buf[plane_idx][idx + HIRES_W] = val;
            self.buf[plane_idx][idx + HIRES_W + 1] = val;
        }
    }

    /// XOR a sprite row onto the display.  Returns true if any lit pixel
    /// was turned off (collision).
    ///
    /// `plane_idx` must be 0 or 1.
    /// `x` and `y` are logical coordinates (already modulo'd by caller to
    /// ensure they start within bounds).  Pixels past the right/bottom edge
    /// are clipped if `clipping` is true, otherwise wrapped.
    pub fn xor_sprite_byte(
        &mut self,
        plane_idx: usize,
        x: usize,
        y: usize,
        byte: u8,
        bits: usize,
        clipping: bool,
    ) -> bool {
        let w = self.width();
        let h = self.height();
        let mut collision = false;
        for bit in 0..bits {
            let px = x + bit;
            // clip or wrap
            let px = if px >= w {
                if clipping {
                    continue;
                } else {
                    px % w
                }
            } else {
                px
            };
            let bit_val = (byte >> (7 - bit)) & 1;
            if bit_val == 0 {
                continue;
            }
            let py = if y >= h {
                continue; // row already out of bounds
            } else {
                y
            };
            let old = self.get(plane_idx, px, py);
            let new_val = old ^ bit_val;
            self.set_pixel(plane_idx, px, py, new_val);
            if old == 1 && new_val == 0 {
                collision = true;
            }
        }
        collision
    }

    /// Scroll down `n` rows in lores (or hires) coordinates.
    pub fn scroll_down(&mut self, n: usize) {
        let w = self.width();
        let h = self.height();
        for p in 0..2 {
            if self.plane & (1 << p) == 0 {
                continue;
            }
            // shift rows downward; clear vacated top rows
            for row in (n..h).rev() {
                for col in 0..w {
                    let from = self.idx(col, row - n);
                    let val = self.buf[p][from];
                    let to = self.idx(col, row);
                    self.buf[p][to] = val;
                    if !self.hires {
                        self.buf[p][to + 1] = val;
                        self.buf[p][to + HIRES_W] = val;
                        self.buf[p][to + HIRES_W + 1] = val;
                    }
                }
            }
            for row in 0..n {
                for col in 0..w {
                    self.set_pixel(p, col, row, 0);
                }
            }
        }
    }

    /// Scroll up `n` rows (XO-CHIP).
    pub fn scroll_up(&mut self, n: usize) {
        let w = self.width();
        let h = self.height();
        for p in 0..2 {
            if self.plane & (1 << p) == 0 {
                continue;
            }
            for row in 0..(h.saturating_sub(n)) {
                for col in 0..w {
                    let from = self.idx(col, row + n);
                    let val = self.buf[p][from];
                    let to = self.idx(col, row);
                    self.buf[p][to] = val;
                    if !self.hires {
                        self.buf[p][to + 1] = val;
                        self.buf[p][to + HIRES_W] = val;
                        self.buf[p][to + HIRES_W + 1] = val;
                    }
                }
            }
            for row in (h.saturating_sub(n))..h {
                for col in 0..w {
                    self.set_pixel(p, col, row, 0);
                }
            }
        }
    }

    /// Scroll right by 4 pixels.
    pub fn scroll_right(&mut self) {
        let w = self.width();
        let h = self.height();
        let shift = 4;
        for p in 0..2 {
            if self.plane & (1 << p) == 0 {
                continue;
            }
            for row in 0..h {
                for col in (shift..w).rev() {
                    let val = self.get(p, col - shift, row);
                    self.set_pixel(p, col, row, val);
                }
                for col in 0..shift {
                    self.set_pixel(p, col, row, 0);
                }
            }
        }
    }

    /// Scroll left by 4 pixels.
    pub fn scroll_left(&mut self) {
        let w = self.width();
        let h = self.height();
        let shift = 4;
        for p in 0..2 {
            if self.plane & (1 << p) == 0 {
                continue;
            }
            for row in 0..h {
                for col in 0..(w - shift) {
                    let val = self.get(p, col + shift, row);
                    self.set_pixel(p, col, row, val);
                }
                for col in (w - shift)..w {
                    self.set_pixel(p, col, row, 0);
                }
            }
        }
    }

    /// Composite the two bitplanes into an RGBA pixel buffer (128×64).
    /// Colour order: bg, fg1, fg2, fg3 (for XO-CHIP 4-colour mode).
    pub fn to_rgba(&self, bg: u32, fg1: u32, fg2: u32, fg3: u32) -> Vec<u32> {
        let mut out = vec![0u32; BUF_SIZE];
        for (i, pixel) in out.iter_mut().enumerate() {
            let p0 = self.buf[0][i];
            let p1 = self.buf[1][i];
            *pixel = match (p0, p1) {
                (0, 0) => bg,
                (1, 0) => fg1,
                (0, 1) => fg2,
                (1, 1) => fg3,
                _ => bg,
            };
        }
        out
    }
}
