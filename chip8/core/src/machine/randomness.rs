/// A simple xorshift64 PRNG for deterministic, per-instance randomness.
///
/// Used by the CHIP-8 `CXNN` (RND) instruction. Each [`crate::cpu::Cpu`]
/// instance owns its own [`Rng`] so multiple engines can run concurrently
/// without interfering with each other.
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Create a new RNG with the given seed. A seed of 0 is mapped to 1 to
    /// prevent entering the all-zero degenerate state.
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    /// Create an RNG directly from a previously saved internal state value.
    /// Used to restore from a [`crate::snapshot::Snapshot`].
    pub fn from_state(state: u64) -> Self {
        Self {
            state: if state == 0 { 1 } else { state },
        }
    }

    /// Generate the next pseudo-random byte (xorshift64).
    #[inline]
    pub fn next_u8(&mut self) -> u8 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        (self.state & 0xFF) as u8
    }

    /// Return the current internal state, for snapshot serialization.
    #[inline]
    pub fn state(&self) -> u64 {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..256 {
            assert_eq!(a.next_u8(), b.next_u8());
        }
    }

    #[test]
    fn different_seeds_differ() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(2);
        let sa: Vec<u8> = (0..64).map(|_| a.next_u8()).collect();
        let sb: Vec<u8> = (0..64).map(|_| b.next_u8()).collect();
        assert_ne!(sa, sb);
    }

    #[test]
    fn restore_from_state() {
        let mut r = Rng::new(99);
        for _ in 0..50 {
            r.next_u8();
        }
        let saved = r.state();
        let expected: Vec<u8> = (0..10).map(|_| r.next_u8()).collect();
        let mut r2 = Rng::from_state(saved);
        let got: Vec<u8> = (0..10).map(|_| r2.next_u8()).collect();
        assert_eq!(expected, got);
    }

    #[test]
    fn zero_seed_does_not_get_stuck() {
        let mut r = Rng::new(0);
        // If state=0 were allowed, xorshift would always produce 0.
        // from_state maps 0→1 so the first output should be non-zero.
        let mut all_zero = true;
        for _ in 0..8 {
            if r.next_u8() != 0 {
                all_zero = false;
                break;
            }
        }
        assert!(!all_zero);
    }
}
