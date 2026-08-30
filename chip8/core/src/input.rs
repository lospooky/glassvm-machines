//! Input scripting for deterministic automated execution.
//!
//! An [`InputScript`] is a sorted list of [`InputEvent`]s that the
//! [`crate::emulator::Engine`] replays frame-by-frame, injecting key presses
//! and releases at the specified frame indices.

/// Whether a key is being pressed or released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum KeyAction {
    Press,
    Release,
}

/// A single scripted key event at a particular frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InputEvent {
    /// Frame index (0-based) at which to apply this action.
    pub frame: u64,
    /// CHIP-8 key index (0x0–0xF).
    pub key: u8,
    /// Whether to press or release the key.
    pub action: KeyAction,
}

/// A sorted sequence of [`InputEvent`]s.
pub type InputScript = Vec<InputEvent>;
