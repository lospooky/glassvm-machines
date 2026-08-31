//! Four packed TIC-80 gamepads.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GamepadState {
    pub mask: u32,
}

impl GamepadState {
    pub const fn pressed(self, player: u8, button: u8) -> bool {
        player < 4 && button < 8 && self.mask & (1_u32 << (player * 8 + button)) != 0
    }
}
