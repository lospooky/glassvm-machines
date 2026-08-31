//! Tolerant artifact-loading boundary for static tools.

use tic80_core::ParsedCart;

pub fn parse(bytes: &[u8]) -> Result<ParsedCart, String> {
    tic80_core::parse_cart(bytes)
}
