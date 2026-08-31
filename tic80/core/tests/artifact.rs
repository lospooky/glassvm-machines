use std::io::Write;

use flate2::{Compression, write::ZlibEncoder};
use tic80_core::{MAX_CART_BYTES, MAX_CODE_BYTES, parse_cart};

const SMOKE: &[u8] = include_bytes!("../../fixtures/smoke.rom");

fn lua_cart(source: &str) -> Vec<u8> {
    let mut cart = vec![5];
    cart.extend_from_slice(&(source.len() as u16).to_le_bytes());
    cart.push(0);
    cart.extend_from_slice(source.as_bytes());
    cart
}

fn compressed_lua_cart(source: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(source).expect("compress source");
    let compressed = encoder.finish().expect("finish compressed source");
    let mut cart = vec![16];
    cart.extend_from_slice(&(compressed.len() as u16).to_le_bytes());
    cart.push(0);
    cart.extend(compressed);
    cart
}

#[test]
fn parses_the_upstream_binary_cartridge() {
    let cartridge = parse_cart(SMOKE).expect("parse native .tic cartridge");
    assert_eq!(cartridge.language, "lua");
    assert!(cartridge.code.contains("function TIC"));
    assert!(!cartridge.chunks.is_empty());
}

#[test]
fn parses_code_and_asset_chunks() {
    let source = "-- script: lua\nfunction TIC() end";
    let mut cart = vec![1, 2, 0, 0, 0x21, 0x43];
    cart.extend(lua_cart(source));

    let parsed = parse_cart(&cart).expect("parse code and tile chunks");

    assert_eq!(parsed.language, "lua");
    assert_eq!(parsed.code, source);
    assert_eq!(parsed.initial_ram()[0x04000], 0x21);
    assert_eq!(parsed.chunks.len(), 2);
}

#[test]
fn cartridge_and_decompressed_source_limits_fail_closed() {
    let oversized_cart = vec![0; MAX_CART_BYTES + 1];
    let error = parse_cart(&oversized_cart).expect_err("oversized cartridge must fail");
    assert!(error.contains("maximum accepted size"));

    let decompression_bomb = compressed_lua_cart(&vec![b'a'; MAX_CODE_BYTES + 1]);
    let error = parse_cart(&decompression_bomb).expect_err("oversized source must fail");
    assert!(error.contains("maximum accepted size"));
}
