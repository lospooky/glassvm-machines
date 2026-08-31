//! Parsed cartridge structure shared by analysis and verification.

use tic80_core::{CartChunk, ParsedCart};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartridgeStructure {
    pub chunks: Vec<CartChunk>,
}

pub fn cartridge_structure(cartridge: &ParsedCart) -> CartridgeStructure {
    CartridgeStructure {
        chunks: cartridge.chunks.clone(),
    }
}
