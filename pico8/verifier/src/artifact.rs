//! Tolerant cartridge loading for static reasoning.

pub use pico8_core::{Cartridge, CartridgeError, CartridgeFormat};

pub fn parse(bytes: &[u8]) -> Result<Cartridge, CartridgeError> {
    Cartridge::parse(bytes)
}
