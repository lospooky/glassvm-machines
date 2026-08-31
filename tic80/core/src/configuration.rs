//! Native machine limits and memory-map configuration.

use std::collections::BTreeMap;

use serde_json::Value;

/// Stable machine identifier shared by native and GlassVM boundaries.
pub const MACHINE_ID: &str = "tic80";

/// Version of the executable machine semantics implemented by this core.
pub const SEMANTICS: &str = "tic80-lua-semantics.v2";
pub const CYCLES_PER_FRAME: u32 = 1;

pub fn validate_runtime_configuration(
    cycles_per_frame: u32,
    machine_params: &BTreeMap<String, Value>,
) -> Result<(), String> {
    if cycles_per_frame != CYCLES_PER_FRAME {
        return Err(format!(
            "TIC-80 requires cycles_per_frame={CYCLES_PER_FRAME}; received {cycles_per_frame}"
        ));
    }
    for key in machine_params.keys() {
        if key != "runtime" {
            return Err(format!("unknown TIC-80 machine parameter {key:?}"));
        }
    }
    match machine_params.get("runtime") {
        None => Ok(()),
        Some(Value::String(runtime)) if runtime == "lua" => Ok(()),
        Some(Value::String(runtime)) => Err(format!(
            "unsupported TIC-80 runtime {runtime:?}; expected \"lua\""
        )),
        Some(value) => Err(format!(
            "TIC-80 machine parameter \"runtime\" must be a string, found {value}"
        )),
    }
}

pub const WIDTH: usize = 240;
pub const HEIGHT: usize = 136;
pub const SCREEN_BYTES: usize = WIDTH * HEIGHT / 2;
pub const VRAM_BYTES: usize = 16 * 1024;
pub const RAM_BYTES: usize = 96 * 1024;

pub(crate) const GAMEPAD_ADDR: usize = 0x0ff80;
pub(crate) const TILES_ADDR: usize = 0x04000;
pub(crate) const SPRITES_ADDR: usize = 0x06000;
pub(crate) const MAP_ADDR: usize = 0x08000;
pub(crate) const FLAGS_ADDR: usize = 0x14404;
pub(crate) const PALETTE_ADDR: usize = 0x03fc0;

pub const MAX_CART_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_CODE_BYTES: usize = 512 * 1024;
pub const LUA_MEMORY_LIMIT_BYTES: usize = 16 * 1024 * 1024;
pub const LUA_INSTRUCTION_BUDGET: u64 = 1_000_000;
pub(crate) const LUA_HOOK_GRANULARITY: u64 = 1_000;
