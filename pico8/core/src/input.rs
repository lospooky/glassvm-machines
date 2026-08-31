//! Native four-player controller masks.

pub const PLAYER_COUNT: usize = 4;
pub const BUTTONS_PER_PLAYER: usize = 6;
pub const CONTROLLER_BITS: usize = PLAYER_COUNT * BUTTONS_PER_PLAYER;

/// Return the bit used by a native player/button pair.
pub fn controller_bit(player: u8, button: u8) -> Option<u32> {
    (player < PLAYER_COUNT as u8 && button < BUTTONS_PER_PLAYER as u8)
        .then_some(u32::from(player) * BUTTONS_PER_PLAYER as u32 + u32::from(button))
}
