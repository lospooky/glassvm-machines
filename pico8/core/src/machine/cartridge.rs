//! PICO-8 cartridge decoding and deterministic machine asset state.
//! by the GlassVM bundle.
//!
//! This crate deliberately keeps the cartridge codec separate from the Lua
//! execution adapter. It supports source `.p8`, encoded `.p8.png`, and raw
//! `.p8.rom` artifacts and materializes the documented 64 KiB address space.

use std::collections::BTreeMap;
use std::io::Cursor;

use sha1::{Digest as _, Sha1};
use thiserror::Error;

pub const RAM_BYTES: usize = 65_536;
pub const CART_DATA_BYTES: usize = 0x4300;
pub const SCREEN_START: usize = 0x6000;
pub const SCREEN_BYTES: usize = 0x2000;
pub const DISPLAY_WIDTH: usize = 128;
pub const DISPLAY_HEIGHT: usize = 128;
pub const MAX_SOURCE_CHARACTERS: usize = 65_535;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CartridgeFormat {
    P8Text,
    P8Png,
    P8Rom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cartridge {
    pub format: CartridgeFormat,
    pub version: u8,
    pub lua: String,
    pub ram: Vec<u8>,
    pub sections: BTreeMap<String, String>,
}

#[derive(Debug, Error)]
pub enum CartridgeError {
    #[error("artifact is empty")]
    Empty,
    #[error("invalid UTF-8 in .p8 cartridge: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("missing PICO-8 cartridge header")]
    MissingHeader,
    #[error("missing or invalid PICO-8 version line")]
    InvalidVersion,
    #[error("missing __lua__ section")]
    MissingLua,
    #[error("invalid hexadecimal digit {character:?} in {section} at line {line}")]
    InvalidHex {
        section: String,
        line: usize,
        character: char,
    },
    #[error("invalid .p8.png cartridge: {0}")]
    Png(String),
    #[error("corrupt .p8.png cartridge: stored SHA-1 does not match decoded payload")]
    PngHashMismatch,
    #[error("compressed Lua program could not be decoded: {0}")]
    Compression(String),
    #[error("unsupported PICO-8 artifact; expected .p8, .p8.png, or 32 KiB .p8.rom")]
    Unsupported,
}

impl Cartridge {
    pub fn parse(bytes: &[u8]) -> Result<Self, CartridgeError> {
        if bytes.is_empty() {
            return Err(CartridgeError::Empty);
        }
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            match parse_png(bytes) {
                Ok(cartridge) => return Ok(cartridge),
                Err(_) if bytes.len() == 0x8000 => return parse_rom(bytes),
                Err(error) => return Err(error),
            }
        }
        if bytes.len() == 0x8000 && !looks_like_text_cartridge(bytes) {
            return parse_rom(bytes);
        }
        if looks_like_text_cartridge(bytes) {
            return parse_text(bytes);
        }
        Err(CartridgeError::Unsupported)
    }

    pub fn parse_as(bytes: &[u8], format: CartridgeFormat) -> Result<Self, CartridgeError> {
        match format {
            CartridgeFormat::P8Text => parse_text(bytes),
            CartridgeFormat::P8Png => parse_png(bytes),
            CartridgeFormat::P8Rom => {
                if bytes.len() != 0x8000 {
                    return Err(CartridgeError::Unsupported);
                }
                parse_rom(bytes)
            }
        }
    }

    pub fn framebuffer(&self) -> Vec<u8> {
        unpack_framebuffer(&self.ram)
    }
}

fn parse_text(bytes: &[u8]) -> Result<Cartridge, CartridgeError> {
    let source = std::str::from_utf8(bytes)?.replace("\r\n", "\n");
    let mut lines = source.lines();
    let header = lines.next().ok_or(CartridgeError::MissingHeader)?;
    if !header.starts_with("pico-8 cartridge") {
        return Err(CartridgeError::MissingHeader);
    }
    let version_line = lines.next().ok_or(CartridgeError::InvalidVersion)?;
    let version = version_line
        .strip_prefix("version ")
        .and_then(|value| value.trim().parse::<u8>().ok())
        .ok_or(CartridgeError::InvalidVersion)?;

    let mut sections: BTreeMap<String, String> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in lines {
        if let Some(name) = line
            .strip_prefix("__")
            .and_then(|value| value.strip_suffix("__"))
            .filter(|value| !value.is_empty())
        {
            current = Some(name.to_ascii_lowercase());
            sections.entry(name.to_ascii_lowercase()).or_default();
        } else if let Some(name) = &current {
            let section = sections.entry(name.clone()).or_default();
            if !section.is_empty() {
                section.push('\n');
            }
            section.push_str(line);
        }
    }
    let lua = sections
        .get("lua")
        .cloned()
        .ok_or(CartridgeError::MissingLua)?;
    let source_characters = lua.chars().count();
    if source_characters > MAX_SOURCE_CHARACTERS {
        return Err(CartridgeError::Compression(format!(
            "source is {source_characters} characters; maximum supported text source is {MAX_SOURCE_CHARACTERS}",
        )));
    }

    let mut ram = default_ram();
    if let Some(gfx) = sections.get("gfx") {
        decode_nibble_pixels(gfx, &mut ram, 0, 128, 128);
    }
    if let Some(gff) = sections.get("gff") {
        decode_hex_rows(gff, "gff", &mut ram, 0x3000, 256)?;
    }
    if let Some(map) = sections.get("map") {
        decode_hex_rows(map, "map", &mut ram, 0x2000, 0x1000)?;
    }
    if let Some(sfx) = sections.get("sfx") {
        decode_sfx(sfx, &mut ram)?;
    }
    if let Some(music) = sections.get("music") {
        decode_music(music, &mut ram)?;
    }

    Ok(Cartridge {
        format: CartridgeFormat::P8Text,
        version,
        lua,
        ram,
        sections,
    })
}

fn parse_png(bytes: &[u8]) -> Result<Cartridge, CartridgeError> {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|error| CartridgeError::Png(error.to_string()))?;
    let info = reader.info();
    if info.width != 160
        || info.height != 205
        || info.color_type != png::ColorType::Rgba
        || info.bit_depth != png::BitDepth::Eight
    {
        return Err(CartridgeError::Png(format!(
            "expected 160x205 8-bit RGBA image, got {}x{} {:?} {:?}",
            info.width, info.height, info.bit_depth, info.color_type
        )));
    }
    let mut rgba = vec![0; reader.output_buffer_size()];
    let frame = reader
        .next_frame(&mut rgba)
        .map_err(|error| CartridgeError::Png(error.to_string()))?;
    rgba.truncate(frame.buffer_size());
    let extracted = pico8_decompress::extract_bits(&rgba);
    if extracted.len() != 0x8020 {
        return Err(CartridgeError::Png(format!(
            "decoded payload has {} bytes; expected 32800",
            extracted.len()
        )));
    }
    let stored_hash = &extracted[0x8006..0x801a];
    if stored_hash.iter().any(|byte| *byte != 0) {
        let actual = Sha1::digest(&extracted[..0x8000]);
        if stored_hash != actual.as_slice() {
            return Err(CartridgeError::PngHashMismatch);
        }
    }
    parse_binary_payload(&extracted, CartridgeFormat::P8Png)
}

fn parse_rom(bytes: &[u8]) -> Result<Cartridge, CartridgeError> {
    parse_binary_payload(bytes, CartridgeFormat::P8Rom)
}

fn parse_binary_payload(
    payload: &[u8],
    format: CartridgeFormat,
) -> Result<Cartridge, CartridgeError> {
    if payload.len() < 0x8000 {
        return Err(CartridgeError::Png(format!(
            "decoded payload has {} bytes; expected at least 32768",
            payload.len()
        )));
    }
    let mut ram = default_ram();
    ram[..CART_DATA_BYTES].copy_from_slice(&payload[..CART_DATA_BYTES]);
    let code_region = &payload[0x4300..0x8000];
    let lua_bytes = decode_code_region(code_region)?;
    let lua = decode_p8scii(&lua_bytes);
    let version = payload.get(0x8000).copied().unwrap_or_default();
    Ok(Cartridge {
        format,
        version,
        lua,
        ram,
        sections: BTreeMap::new(),
    })
}

fn decode_code_region(region: &[u8]) -> Result<Vec<u8>, CartridgeError> {
    if region.starts_with(b"\0pxa") {
        return decode_pxa(region).map_err(CartridgeError::Compression);
    }
    if region.starts_with(b":c:\0") {
        let mut output = vec![0; MAX_SOURCE_CHARACTERS + 1];
        let size = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pico8_decompress::p8::decompress(region, &mut output)
        }))
        .map_err(|_| {
            CartridgeError::Compression("malformed legacy stream caused decoder failure".into())
        })?
        .map_err(|error| CartridgeError::Compression(error.to_string()))?;
        output.truncate(size);
        return Ok(output);
    }
    let end = region
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(region.len());
    Ok(region[..end].to_vec())
}

fn decode_pxa(source: &[u8]) -> Result<Vec<u8>, String> {
    const MIN_BLOCK: usize = 3;
    let header = source
        .get(..8)
        .ok_or_else(|| "truncated PXA header".to_string())?;
    if &header[..4] != b"\0pxa" {
        return Err("invalid PXA header".into());
    }
    let raw_len = usize::from(u16::from_be_bytes([header[4], header[5]]));
    let compressed_len = usize::from(u16::from_be_bytes([header[6], header[7]]));
    if raw_len > MAX_SOURCE_CHARACTERS {
        return Err(format!(
            "PXA declares {raw_len} output characters; maximum is {MAX_SOURCE_CHARACTERS}"
        ));
    }
    if !(8..=source.len()).contains(&compressed_len) {
        return Err(format!(
            "PXA compressed length {compressed_len} is outside the available {} bytes",
            source.len()
        ));
    }
    let mut bits = PxaBits::new(&source[..compressed_len]);
    for _ in 0..8 {
        bits.value(8)?;
    }

    let mut output = Vec::with_capacity(raw_len);
    let mut literals: Vec<u8> = (0..=255).map(|value| value as u8).collect();
    while output.len() < raw_len {
        if bits.byte_position() >= compressed_len {
            return Err(format!(
                "PXA stream ended after {} of {raw_len} output bytes",
                output.len()
            ));
        }
        if !bits.bit()? {
            let distance_kind = pxa_number(&mut bits)?;
            if let Some(distance) = distance_kind {
                let block_offset = distance
                    .checked_add(1)
                    .ok_or_else(|| "PXA block offset overflow".to_string())?;
                if block_offset > output.len() {
                    return Err(format!(
                        "PXA block offset {block_offset} exceeds {} decoded bytes",
                        output.len()
                    ));
                }
                let block_len = pxa_chain(&mut bits, 3, 100_000)?
                    .checked_add(MIN_BLOCK)
                    .ok_or_else(|| "PXA block length overflow".to_string())?;
                if block_len > raw_len - output.len() {
                    return Err(format!(
                        "PXA block length {block_len} exceeds remaining output {}",
                        raw_len - output.len()
                    ));
                }
                for _ in 0..block_len {
                    let value = output[output.len() - block_offset];
                    output.push(value);
                }
            } else {
                while output.len() < raw_len {
                    let value = bits.value(8)? as u8;
                    output.push(value);
                }
            }
        } else {
            let mut literal_position = 0_usize;
            let mut extra_bits = 0_usize;
            while bits.bit()? {
                if extra_bits >= 16 {
                    return Err("PXA literal prefix exceeds 16 bits".into());
                }
                literal_position = literal_position
                    .checked_add(1 << (4 + extra_bits))
                    .ok_or_else(|| "PXA literal position overflow".to_string())?;
                extra_bits += 1;
            }
            literal_position = literal_position
                .checked_add(bits.value(4 + extra_bits)?)
                .ok_or_else(|| "PXA literal position overflow".to_string())?;
            let value = *literals
                .get(literal_position)
                .ok_or_else(|| format!("PXA literal position {literal_position} exceeds 255"))?;
            output.push(value);
            literals.remove(literal_position);
            literals.insert(0, value);
        }
    }
    Ok(output)
}

fn pxa_number(bits: &mut PxaBits<'_>) -> Result<Option<usize>, String> {
    let prefix = pxa_chain(bits, 1, 2)?;
    let width = (3_usize.saturating_sub(prefix)) * 5;
    let value = bits.value(width)?;
    if value == 0 && width == 10 {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn pxa_chain(bits: &mut PxaBits<'_>, link_bits: usize, max_bits: usize) -> Result<usize, String> {
    let max_value = (1 << link_bits) - 1;
    let mut value = 0_usize;
    let mut read = 0_usize;
    loop {
        let part = bits.value(link_bits)?;
        value = value
            .checked_add(part)
            .ok_or_else(|| "PXA chain overflow".to_string())?;
        read += link_bits;
        if part != max_value || read >= max_bits {
            return Ok(value);
        }
    }
}

struct PxaBits<'a> {
    bytes: &'a [u8],
    bit_index: usize,
}

impl<'a> PxaBits<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            bit_index: 0,
        }
    }

    fn byte_position(&self) -> usize {
        self.bit_index / 8
    }

    fn bit(&mut self) -> Result<bool, String> {
        let byte = self
            .bytes
            .get(self.byte_position())
            .ok_or_else(|| "unexpected end of PXA bitstream".to_string())?;
        let value = byte & (1 << (self.bit_index % 8)) != 0;
        self.bit_index += 1;
        Ok(value)
    }

    fn value(&mut self, width: usize) -> Result<usize, String> {
        if width > 20 {
            return Err(format!("unsupported PXA value width {width}"));
        }
        let mut value = 0;
        for bit in 0..width {
            if self.bit()? {
                value |= 1 << bit;
            }
        }
        Ok(value)
    }
}

fn hex(character: char, section: &str, line: usize) -> Result<u8, CartridgeError> {
    character
        .to_digit(16)
        .map(|value| value as u8)
        .ok_or_else(|| CartridgeError::InvalidHex {
            section: section.into(),
            line,
            character,
        })
}

fn pixel_hex(character: char) -> u8 {
    character.to_digit(16).unwrap_or(0) as u8
}

fn decode_nibble_pixels(source: &str, ram: &mut [u8], start: usize, width: usize, max_rows: usize) {
    for (y, line) in source.lines().take(max_rows).enumerate() {
        for (x, character) in line.chars().take(width).enumerate() {
            let value = pixel_hex(character);
            let address = start + y * (width / 2) + x / 2;
            if x % 2 == 0 {
                ram[address] = (ram[address] & 0xf0) | value;
            } else {
                ram[address] = (ram[address] & 0x0f) | (value << 4);
            }
        }
    }
}

fn default_ram() -> Vec<u8> {
    let mut ram = vec![0; RAM_BYTES];
    for pattern in 0..64 {
        let base = 0x3100 + pattern * 4;
        ram[base..base + 4].copy_from_slice(&[0x41, 0x42, 0x43, 0x44]);
    }
    for sfx in 0..64 {
        let base = 0x3200 + sfx * 68 + 64;
        ram[base..base + 4].copy_from_slice(if sfx == 0 {
            &[0x00, 0x01, 0x00, 0x00]
        } else {
            &[0x00, 0x10, 0x00, 0x00]
        });
    }
    ram
}

fn looks_like_text_cartridge(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok_and(|source| {
        let mut lines = source.lines();
        lines
            .next()
            .is_some_and(|line| line.starts_with("pico-8 cartridge //"))
            && lines
                .next()
                .and_then(|line| line.strip_prefix("version "))
                .is_some_and(|version| version.trim().parse::<u8>().is_ok())
            && source.contains("\n__lua__")
    })
}

fn decode_p8scii(bytes: &[u8]) -> String {
    const PUNCTUATION: [&str; 16] = [
        "▮", "■", "□", "⁙", "⁘", "‖", "◀", "▶", "「", "」", "¥", "•", "、", "。", "゛", "゜",
    ];
    const EXTENDED: [&str; 129] = [
        "○", "█", "▒", "🐱", "⬇️", "░", "✽", "●", "♥", "☉", "웃", "⌂", "⬅️", "😐", "♪", "🅾️", "◆",
        "…", "➡️", "★", "⧗", "⬆️", "ˇ", "∧", "❎", "▤", "▥", "あ", "い", "う", "え", "お", "か",
        "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と", "な",
        "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も", "や",
        "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "を", "ん", "っ", "ゃ", "ゅ", "ょ", "ア",
        "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ",
        "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ",
        "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "ヲ", "ン",
        "ッ", "ャ", "ュ", "ョ", "◜", "◝",
    ];
    let mut output = String::new();
    for byte in bytes {
        match *byte {
            0..=15 => output.push(char::from(*byte)),
            16..=31 => output.push_str(PUNCTUATION[usize::from(*byte - 16)]),
            32..=126 => output.push(char::from(*byte)),
            127..=255 => output.push_str(EXTENDED[usize::from(*byte - 127)]),
        }
    }
    output
}

fn decode_hex_rows(
    source: &str,
    section: &str,
    ram: &mut [u8],
    start: usize,
    max_bytes: usize,
) -> Result<(), CartridgeError> {
    let mut cursor = 0;
    for (line_number, line) in source.lines().enumerate() {
        let characters: Vec<char> = line
            .chars()
            .filter(|value| !value.is_whitespace())
            .collect();
        for pair in characters.chunks(2) {
            if pair.len() != 2 || cursor >= max_bytes {
                break;
            }
            ram[start + cursor] = (hex(pair[0], section, line_number + 1)? << 4)
                | hex(pair[1], section, line_number + 1)?;
            cursor += 1;
        }
        if cursor >= max_bytes {
            break;
        }
    }
    Ok(())
}

fn decode_sfx(source: &str, ram: &mut [u8]) -> Result<(), CartridgeError> {
    for (sfx_index, line) in source.lines().take(64).enumerate() {
        let chars: Vec<char> = line.chars().collect();
        if chars.len() < 8 {
            continue;
        }
        let base = 0x3200 + sfx_index * 68;
        for note in 0..32 {
            let offset = 8 + note * 5;
            if offset + 5 > chars.len() {
                break;
            }
            let pitch = (hex(chars[offset], "sfx", sfx_index + 1)? << 4)
                | hex(chars[offset + 1], "sfx", sfx_index + 1)?;
            let waveform = hex(chars[offset + 2], "sfx", sfx_index + 1)?;
            let volume = hex(chars[offset + 3], "sfx", sfx_index + 1)?;
            let effect = hex(chars[offset + 4], "sfx", sfx_index + 1)?;
            let packed = (pitch as u16 & 0x3f)
                | ((waveform as u16 & 0x7) << 6)
                | ((volume as u16 & 0x7) << 9)
                | ((effect as u16 & 0x7) << 12)
                | ((waveform as u16 & 0x8) << 12);
            ram[base + note * 2] = packed as u8;
            ram[base + note * 2 + 1] = (packed >> 8) as u8;
        }
        for header in 0..4 {
            ram[base + 64 + header] = (hex(chars[header * 2], "sfx", sfx_index + 1)? << 4)
                | hex(chars[header * 2 + 1], "sfx", sfx_index + 1)?;
        }
    }
    Ok(())
}

fn decode_music(source: &str, ram: &mut [u8]) -> Result<(), CartridgeError> {
    for (pattern, line) in source.lines().take(64).enumerate() {
        let compact: Vec<char> = line
            .chars()
            .filter(|value| !value.is_whitespace())
            .collect();
        if compact.len() < 10 {
            continue;
        }
        let flags =
            (hex(compact[0], "music", pattern + 1)? << 4) | hex(compact[1], "music", pattern + 1)?;
        for channel in 0..4 {
            let offset = 2 + channel * 2;
            let mut value = (hex(compact[offset], "music", pattern + 1)? << 4)
                | hex(compact[offset + 1], "music", pattern + 1)?;
            if channel < 3 && flags & (1 << channel) != 0 {
                value |= 0x80;
            }
            ram[0x3100 + pattern * 4 + channel] = value;
        }
    }
    Ok(())
}

pub fn get_pixel(ram: &[u8], x: i32, y: i32) -> u8 {
    if !(0..DISPLAY_WIDTH as i32).contains(&x) || !(0..DISPLAY_HEIGHT as i32).contains(&y) {
        return 0;
    }
    let index = SCREEN_START + y as usize * 64 + x as usize / 2;
    if x & 1 == 0 {
        ram[index] & 0x0f
    } else {
        ram[index] >> 4
    }
}

pub fn set_pixel(ram: &mut [u8], x: i32, y: i32, color: u8) {
    if !(0..DISPLAY_WIDTH as i32).contains(&x) || !(0..DISPLAY_HEIGHT as i32).contains(&y) {
        return;
    }
    let index = SCREEN_START + y as usize * 64 + x as usize / 2;
    if x & 1 == 0 {
        ram[index] = (ram[index] & 0xf0) | (color & 0x0f);
    } else {
        ram[index] = (ram[index] & 0x0f) | ((color & 0x0f) << 4);
    }
}

pub fn unpack_framebuffer(ram: &[u8]) -> Vec<u8> {
    let mut pixels = vec![0; DISPLAY_WIDTH * DISPLAY_HEIGHT];
    for y in 0..DISPLAY_HEIGHT {
        for x in 0..DISPLAY_WIDTH {
            pixels[y * DISPLAY_WIDTH + x] = get_pixel(ram, x as i32, y as i32);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_text_cart_and_materializes_assets() {
        let source = b"pico-8 cartridge // http://www.pico-8.com\nversion 42\n__lua__\nfunction _draw() cls(2) end\n__gfx__\n1234\n__gff__\n0aff\n__map__\n0102\n";
        let cart = Cartridge::parse(source).expect("parse");
        assert_eq!(cart.format, CartridgeFormat::P8Text);
        assert_eq!(cart.version, 42);
        assert_eq!(cart.ram[0], 0x21);
        assert_eq!(cart.ram[1], 0x43);
        assert_eq!(&cart.ram[0x3000..0x3002], &[0x0a, 0xff]);
        assert_eq!(&cart.ram[0x2000..0x2002], &[1, 2]);
    }

    #[test]
    fn screen_pixels_use_pico8_nibble_order() {
        let mut ram = vec![0; RAM_BYTES];
        set_pixel(&mut ram, 0, 0, 3);
        set_pixel(&mut ram, 1, 0, 12);
        assert_eq!(ram[SCREEN_START], 0xc3);
        assert_eq!(get_pixel(&ram, 0, 0), 3);
        assert_eq!(get_pixel(&ram, 1, 0), 12);
    }

    #[test]
    fn rejects_non_cartridge_bytes() {
        assert!(matches!(
            Cartridge::parse(b"not a cartridge"),
            Err(CartridgeError::Unsupported)
        ));
    }

    #[test]
    fn empty_text_cart_receives_documented_default_audio_data() {
        let source = b"pico-8 cartridge // http://www.pico-8.com\nversion 42\n__lua__\n";
        let cart = Cartridge::parse(source).expect("parse");
        assert_eq!(&cart.ram[0x3100..0x3104], &[0x41, 0x42, 0x43, 0x44]);
        assert_eq!(&cart.ram[0x3240..0x3244], &[0x00, 0x01, 0x00, 0x00]);
        assert_eq!(&cart.ram[0x3284..0x3288], &[0x00, 0x10, 0x00, 0x00]);
    }

    #[test]
    fn text_sfx_and_music_convert_to_memory_layout() {
        let notes = format!("05876{}", "00000".repeat(31));
        let source = format!(
            "pico-8 cartridge // http://www.pico-8.com\nversion 42\n__lua__\n__sfx__\n01020304{notes}\n__music__\n07 00414203\n"
        );
        let cart = Cartridge::parse(source.as_bytes()).expect("parse");
        assert_eq!(&cart.ram[0x3200..0x3202], &[0x05, 0xee]);
        assert_eq!(&cart.ram[0x3240..0x3244], &[1, 2, 3, 4]);
        assert_eq!(&cart.ram[0x3100..0x3104], &[0x80, 0xc1, 0xc2, 0x03]);
    }

    #[test]
    fn invalid_gfx_characters_decode_as_colour_zero() {
        let source =
            b"pico-8 cartridge // http://www.pico-8.com\nversion 42\n__lua__\n__gfx__\nz1\n";
        let cart = Cartridge::parse(source).expect("parse");
        assert_eq!(cart.ram[0], 0x10);
    }

    #[test]
    fn source_png_and_rom_fixtures_decode_equivalently() {
        let text = Cartridge::parse(include_bytes!("../../../fixtures/source/smoke.p8"))
            .expect("text cart");
        let png = Cartridge::parse(include_bytes!("../../../fixtures/cases/reference.p8.png"))
            .expect("png cart");
        let rom = Cartridge::parse_as(
            include_bytes!("../../../fixtures/cases/reference.p8.rom"),
            CartridgeFormat::P8Rom,
        )
        .expect("rom cart");
        assert_eq!(text.lua.trim(), png.lua.trim());
        assert_eq!(png.lua, rom.lua);
        assert_eq!(&text.ram[..CART_DATA_BYTES], &png.ram[..CART_DATA_BYTES]);
        assert_eq!(&png.ram[..CART_DATA_BYTES], &rom.ram[..CART_DATA_BYTES]);
        assert_eq!(png.version, 41);
    }

    #[test]
    fn png_hash_mismatch_is_rejected() {
        let bytes = include_bytes!("../../../fixtures/cases/reference.p8.png");
        let decoder = png::Decoder::new(Cursor::new(bytes));
        let mut reader = decoder.read_info().expect("png metadata");
        let mut rgba = vec![0; reader.output_buffer_size()];
        let frame = reader.next_frame(&mut rgba).expect("png frame");
        rgba.truncate(frame.buffer_size());
        rgba[0x8006 * 4 + 2] ^= 1;

        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 160, 205);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            writer.write_image_data(&rgba).expect("pixels");
        }
        assert!(matches!(
            Cartridge::parse(&encoded),
            Err(CartridgeError::PngHashMismatch)
        ));
    }

    #[test]
    fn binary_p8scii_source_is_not_rejected_as_utf8() {
        let mut rom = vec![0; 0x8000];
        rom[0x4300..0x4306].copy_from_slice(&[b'-', b'-', b' ', 0x80, 0x82, 0]);
        let cart = Cartridge::parse_as(&rom, CartridgeFormat::P8Rom).expect("rom");
        assert_eq!(cart.lua, "-- █🐱");
    }

    #[test]
    fn legacy_compressed_code_is_supported() {
        let mut rom = vec![0; 0x8000];
        rom[0x4300..0x4310].copy_from_slice(&[
            b':', b'c', b':', 0, 0, 8, 0, 0, 28, 30, 21, 26, 32, 42, 4, 43,
        ]);
        let cart = Cartridge::parse_as(&rom, CartridgeFormat::P8Rom).expect("legacy ROM");
        assert_eq!(cart.lua, "print(1)");
    }

    #[test]
    fn malformed_pxa_returns_error_instead_of_unwinding() {
        let mut rom = vec![0; 0x8000];
        rom[0x4300..0x4309].copy_from_slice(&[0, b'p', b'x', b'a', 0, 1, 0, 9, 0]);
        let result = std::panic::catch_unwind(|| Cartridge::parse_as(&rom, CartridgeFormat::P8Rom));
        assert!(result.is_ok(), "parser unwound");
        assert!(matches!(
            result.expect("no unwind"),
            Err(CartridgeError::Compression(_))
        ));
    }

    #[test]
    fn text_source_limit_counts_characters_not_utf8_bytes() {
        let source = format!(
            "pico-8 cartridge // http://www.pico-8.com\nversion 42\n__lua__\n--{}\n",
            "🐱".repeat(16_384)
        );
        Cartridge::parse(source.as_bytes()).expect("valid character count");
    }

    #[test]
    fn png_requires_documented_carrier_geometry() {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            writer.write_image_data(&[0, 0, 0, 255]).expect("pixel");
        }
        assert!(matches!(
            Cartridge::parse(&encoded),
            Err(CartridgeError::Png(_))
        ));
    }

    #[test]
    fn explicit_rom_format_resolves_magic_prefix_collisions() {
        let mut rom = vec![0; 0x8000];
        rom[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        rom[0x4300..0x4309].copy_from_slice(b"print(1)\0");
        let cart = Cartridge::parse(&rom).expect("automatic ROM fallback");
        assert_eq!(cart.format, CartridgeFormat::P8Rom);
        assert_eq!(cart.lua, "print(1)");
    }
}
