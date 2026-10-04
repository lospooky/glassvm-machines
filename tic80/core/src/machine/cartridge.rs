use std::io::Read;

use flate2::read::ZlibDecoder;
use serde::{Deserialize, Serialize};

use crate::configuration::{
    FLAGS_ADDR, MAP_ADDR, MAX_CART_BYTES, MAX_CODE_BYTES, PALETTE_ADDR, PALETTE_MAP_ADDR,
    RAM_BYTES, SPRITES_ADDR, TILES_ADDR, VRAM_BYTES,
};

use super::SWEETIE_16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CartChunk {
    pub kind: u8,
    pub bank: u8,
    pub size: usize,
}

#[derive(Debug, Clone)]
pub struct ParsedCart {
    pub code: String,
    pub language: String,
    pub chunks: Vec<CartChunk>,
    pub(super) initial_ram: Vec<u8>,
}

impl ParsedCart {
    pub fn initial_ram(&self) -> &[u8] {
        &self.initial_ram
    }
}

/// Parse an official binary `.tic` cartridge.
pub fn parse_cart(bytes: &[u8]) -> Result<ParsedCart, String> {
    if bytes.is_empty() {
        return Err("empty TIC-80 cartridge".into());
    }
    if bytes.len() > MAX_CART_BYTES {
        return Err(format!(
            "TIC-80 cartridge is {} bytes; maximum accepted size is {MAX_CART_BYTES}",
            bytes.len()
        ));
    }

    let mut offset = 0usize;
    let mut chunks = Vec::new();
    let mut code_banks: [Option<Vec<u8>>; 8] = Default::default();
    let mut compressed_code = None;
    let mut language_chunk = None;
    let mut ram = vec![0u8; RAM_BYTES];
    install_default_palette(&mut ram);
    install_default_palette_map(&mut ram);

    while offset < bytes.len() {
        if bytes.len() - offset < 4 {
            return Err(format!("truncated chunk header at byte {offset}"));
        }
        let tag = bytes[offset];
        let kind = tag & 0x1f;
        let bank = tag >> 5;
        let encoded_size = u16::from_le_bytes([bytes[offset + 1], bytes[offset + 2]]) as usize;
        offset += 4;

        let size = if encoded_size == 0
            && matches!(kind, 5 | 19)
            && bytes.len().saturating_sub(offset) >= 65_536
        {
            65_536
        } else {
            encoded_size
        };
        if size > bytes.len() - offset {
            return Err(format!(
                "chunk type {kind} bank {bank} declares {size} bytes with only {} remaining",
                bytes.len() - offset
            ));
        }
        let data = &bytes[offset..offset + size];
        offset += size;
        chunks.push(CartChunk { kind, bank, size });

        match kind {
            1 => copy_asset(&mut ram, TILES_ADDR, data, 8192),
            2 => copy_asset(&mut ram, SPRITES_ADDR, data, 8192),
            4 => copy_asset(&mut ram, MAP_ADDR, data, 32_640),
            5 => code_banks[bank as usize] = Some(data.to_vec()),
            6 => copy_asset(&mut ram, FLAGS_ADDR, data, 512),
            12 if bank == 0 => copy_asset(&mut ram, PALETTE_ADDR, data, 48),
            16 => compressed_code = Some(data.to_vec()),
            18 if bank == 0 => copy_asset(&mut ram, 0, data, VRAM_BYTES),
            20 => language_chunk = data.first().copied(),
            _ => {}
        }
    }

    let code_bytes = if code_banks.iter().any(Option::is_some) {
        code_banks
            .into_iter()
            .flatten()
            .flatten()
            .collect::<Vec<_>>()
    } else if let Some(compressed) = compressed_code {
        let mut decoded = Vec::new();
        ZlibDecoder::new(compressed.as_slice())
            .take((MAX_CODE_BYTES + 1) as u64)
            .read_to_end(&mut decoded)
            .map_err(|error| format!("invalid compressed TIC-80 code: {error}"))?;
        decoded
    } else {
        return Err("cartridge has no source-code chunk".into());
    };
    if code_bytes.len() > MAX_CODE_BYTES {
        return Err(format!(
            "TIC-80 source code expands to {} bytes; maximum accepted size is {MAX_CODE_BYTES}",
            code_bytes.len()
        ));
    }
    let code = String::from_utf8(code_bytes)
        .map_err(|error| format!("TIC-80 source code is not UTF-8: {error}"))?;
    let language = detect_language(&code, language_chunk);

    Ok(ParsedCart {
        code,
        language,
        chunks,
        initial_ram: ram,
    })
}

fn copy_asset(ram: &mut [u8], start: usize, data: &[u8], maximum: usize) {
    let count = data.len().min(maximum).min(ram.len().saturating_sub(start));
    ram[start..start + count].copy_from_slice(&data[..count]);
}

fn install_default_palette(ram: &mut [u8]) {
    for (index, color) in SWEETIE_16.iter().enumerate() {
        ram[PALETTE_ADDR + index * 3..PALETTE_ADDR + index * 3 + 3].copy_from_slice(color);
    }
}

fn install_default_palette_map(ram: &mut [u8]) {
    for index in 0..8 {
        ram[PALETTE_MAP_ADDR + index] = (index as u8 * 2) | ((index as u8 * 2 + 1) << 4);
    }
}

fn detect_language(code: &str, language_chunk: Option<u8>) -> String {
    let prefix = code
        .lines()
        .take(16)
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    let markers = [
        ("script: javascript", "javascript"),
        ("script: js", "javascript"),
        ("script: python", "python"),
        ("script: ruby", "ruby"),
        ("script: wren", "wren"),
        ("script: squirrel", "squirrel"),
        ("script: fennel", "fennel"),
        ("script: moon", "moonscript"),
        ("script: scheme", "scheme"),
        ("script: janet", "janet"),
        ("script: wasm", "wasm"),
        ("script: lua", "lua"),
    ];
    if let Some((_, language)) = markers.iter().find(|(marker, _)| prefix.contains(marker)) {
        return (*language).into();
    }
    match language_chunk {
        // The upstream enum has changed over time. Only zero is treated as the
        // stable default; non-zero values remain explicit rather than guessed.
        None | Some(0) => "lua".into(),
        Some(value) => format!("tic80-language-id-{value}"),
    }
}
