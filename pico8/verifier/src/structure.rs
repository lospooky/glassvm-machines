//! Cartridge structure shared by analyzer and verification rules.

use pico8_core::{Cartridge, CartridgeFormat};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartridgeStructure {
    pub sections: Vec<String>,
}

pub fn cartridge_structure(cartridge: &Cartridge) -> CartridgeStructure {
    CartridgeStructure {
        sections: cartridge.sections.keys().cloned().collect(),
    }
}

pub const fn artifact_format_name(format: CartridgeFormat) -> &'static str {
    match format {
        CartridgeFormat::P8Text => "p8-text",
        CartridgeFormat::P8Png => "p8-png",
        CartridgeFormat::P8Rom => "p8-rom",
    }
}
