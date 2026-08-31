use std::f64::consts::TAU;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::{
    Cartridge, DISPLAY_HEIGHT, DISPLAY_WIDTH, RAM_BYTES, SCREEN_BYTES, SCREEN_START, get_pixel,
    set_pixel, unpack_framebuffer,
};
use mlua::{
    Function, HookTriggers, Lua, LuaOptions, MultiValue, StdLib, Table, Value, Variadic, VmState,
};
use serde::{Deserialize, Serialize};

const DEFAULT_LUA_MEMORY_BYTES: usize = 2 * 1024 * 1024;
const HOOK_GRANULARITY: u64 = 1_000;
const MAX_PRINT_LOG_ENTRIES: usize = 4_096;
const MAX_PRINT_ENTRY_BYTES: usize = 4_096;

type SprArgs = (
    i64,
    i32,
    i32,
    Option<i32>,
    Option<i32>,
    Option<bool>,
    Option<bool>,
);
type SsprArgs = (
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    Option<i32>,
    Option<i32>,
    Option<bool>,
    Option<bool>,
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSnapshot {
    pub schema_version: u32,
    pub ram: Vec<u8>,
    pub frame: u64,
    pub input_mask: u16,
    pub previous_input_mask: u16,
    pub rng_state: u64,
    pub callback_hz: u32,
    pub draw_color: u8,
    pub draw_palette: [u8; 16],
    pub display_palette: [u8; 16],
    pub transparent: [bool; 16],
    pub camera_x: i32,
    pub camera_y: i32,
    pub clip: [i32; 4],
    pub draw_calls: u64,
    pub audio_calls: u64,
    pub printed: Vec<String>,
}

impl RuntimeSnapshot {
    fn new(cartridge: &Cartridge, seed: u64) -> Self {
        Self {
            schema_version: 1,
            ram: cartridge.ram.clone(),
            frame: 0,
            input_mask: 0,
            previous_input_mask: 0,
            rng_state: seed ^ 0x9e37_79b9_7f4a_7c15,
            callback_hz: 30,
            draw_color: 6,
            draw_palette: std::array::from_fn(|index| index as u8),
            display_palette: std::array::from_fn(|index| index as u8),
            transparent: std::array::from_fn(|index| index == 0),
            camera_x: 0,
            camera_y: 0,
            clip: [0, 0, DISPLAY_WIDTH as i32, DISPLAY_HEIGHT as i32],
            draw_calls: 0,
            audio_calls: 0,
            printed: Vec::new(),
        }
    }
}

pub struct Pico8Runtime {
    lua: Lua,
    state: Arc<Mutex<RuntimeSnapshot>>,
    instructions_remaining: Arc<AtomicU64>,
    instruction_budget: u64,
    fps: u32,
}

impl Pico8Runtime {
    pub fn new(cartridge: &Cartridge, seed: u64, instruction_budget: u64) -> Result<Self, String> {
        if instruction_budget < HOOK_GRANULARITY {
            return Err(format!(
                "instruction budget must be at least {HOOK_GRANULARITY}"
            ));
        }
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING,
            LuaOptions::new().catch_rust_panics(false),
        )
        .map_err(|error| format!("cannot construct sandboxed PICO-8 Lua state: {error}"))?;
        lua.set_memory_limit(DEFAULT_LUA_MEMORY_BYTES)
            .map_err(|error| format!("cannot set PICO-8 Lua memory limit: {error}"))?;

        let state = Arc::new(Mutex::new(RuntimeSnapshot::new(cartridge, seed)));
        install_sandbox(&lua)?;
        install_api(&lua, Arc::clone(&state))?;

        let instructions_remaining = Arc::new(AtomicU64::new(instruction_budget));
        let hook_budget = Arc::clone(&instructions_remaining);
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(HOOK_GRANULARITY as u32),
            move |_lua, _debug| {
                let previous =
                    hook_budget.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                        remaining.checked_sub(HOOK_GRANULARITY)
                    });
                if previous.is_err() {
                    return Err(mlua::Error::RuntimeError(
                        "PICO-8 instruction budget exhausted".into(),
                    ));
                }
                Ok(VmState::Continue)
            },
        );

        let patched = pico8_to_lua::patch_lua(cartridge.lua.as_str());
        lua.load(patched.as_ref())
            .set_name("cartridge")
            .exec()
            .map_err(|error| format!("PICO-8 Lua load failed: {error}"))?;

        let globals = lua.globals();
        let fps = if matches!(globals.get::<Value>("_update60"), Ok(Value::Function(_))) {
            60
        } else {
            30
        };
        lock(&state).callback_hz = fps;
        if let Ok(init) = globals.get::<Function>("_init") {
            instructions_remaining.store(instruction_budget, Ordering::Relaxed);
            init.call::<()>(())
                .map_err(|error| format!("PICO-8 _init failed: {error}"))?;
        }
        drop(globals);

        Ok(Self {
            lua,
            state,
            instructions_remaining,
            instruction_budget,
            fps,
        })
    }

    pub fn fps(&self) -> u32 {
        self.fps
    }

    pub fn frame(&self) -> u64 {
        lock(&self.state).frame
    }

    pub fn set_input_mask(&self, mask: u16) {
        lock(&self.state).input_mask = mask;
    }

    pub fn step_frame(&self) -> Result<(), String> {
        self.instructions_remaining
            .store(self.instruction_budget, Ordering::Relaxed);
        let globals = self.lua.globals();
        if self.fps == 60 {
            call_optional(&globals, "_update60")?;
        } else {
            call_optional(&globals, "_update")?;
        }
        call_optional(&globals, "_draw")?;
        drop(globals);

        let mut state = lock(&self.state);
        state.previous_input_mask = state.input_mask;
        state.frame = state.frame.saturating_add(1);
        Ok(())
    }

    pub fn snapshot(&self) -> RuntimeSnapshot {
        lock(&self.state).clone()
    }

    /// Restore the current machine state without reconstructing a run from
    /// observational history. The Lua API closes over this shared state, so
    /// replacing it is sufficient for the state represented by this native
    /// runtime snapshot.
    pub fn restore_machine_state(&self, snapshot: &RuntimeSnapshot) -> Result<(), String> {
        if snapshot.schema_version != 1 {
            return Err(format!(
                "unsupported PICO-8 native machine-state version {}; expected 1",
                snapshot.schema_version
            ));
        }
        if snapshot.ram.len() != RAM_BYTES {
            return Err(format!(
                "PICO-8 native machine state has {} RAM bytes; expected {RAM_BYTES}",
                snapshot.ram.len()
            ));
        }
        *lock(&self.state) = snapshot.clone();
        Ok(())
    }

    pub fn framebuffer(&self) -> Vec<u8> {
        let state = lock(&self.state);
        unpack_framebuffer(&state.ram)
            .into_iter()
            .map(|color| state.display_palette[color as usize & 0x0f])
            .collect()
    }
}

fn call_optional(globals: &Table, name: &str) -> Result<(), String> {
    match globals.get::<Value>(name) {
        Ok(Value::Function(function)) => function
            .call::<()>(())
            .map_err(|error| format!("PICO-8 {name} failed: {error}")),
        Ok(_) | Err(_) => Ok(()),
    }
}

fn lock(state: &Arc<Mutex<RuntimeSnapshot>>) -> MutexGuard<'_, RuntimeSnapshot> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn install_sandbox(lua: &Lua) -> Result<(), String> {
    let globals = lua.globals();
    for name in [
        "collectgarbage",
        "debug",
        "dofile",
        "io",
        "load",
        "loadfile",
        "os",
        "package",
        "pcall",
        "require",
        "xpcall",
    ] {
        globals
            .set(name, Value::Nil)
            .map_err(|error| format!("cannot sandbox Lua global {name}: {error}"))?;
    }
    Ok(())
}

fn install_api(lua: &Lua, state: Arc<Mutex<RuntimeSnapshot>>) -> Result<(), String> {
    let globals = lua.globals();

    set_function(
        lua,
        &globals,
        "cls",
        clone_state(&state, |state, color: Option<i64>| {
            let color = color.unwrap_or(0) as u8 & 0x0f;
            state.ram[SCREEN_START..SCREEN_START + SCREEN_BYTES].fill(color | (color << 4));
            state.draw_calls += 1;
            Ok(())
        }),
    )?;
    set_function(
        lua,
        &globals,
        "pset",
        clone_state(&state, |state, (x, y, color): (i32, i32, Option<i64>)| {
            draw_pixel(state, x, y, color.unwrap_or(state.draw_color as i64) as u8);
            state.draw_calls += 1;
            Ok(())
        }),
    )?;
    set_function(
        lua,
        &globals,
        "pget",
        clone_state(&state, |state, (x, y): (i32, i32)| {
            Ok(get_pixel(&state.ram, x, y) as i64)
        }),
    )?;
    set_function(
        lua,
        &globals,
        "rectfill",
        clone_state(
            &state,
            |state, (x0, y0, x1, y1, color): (i32, i32, i32, i32, Option<i64>)| {
                let color = color.unwrap_or(state.draw_color as i64) as u8;
                let [clip_x, clip_y, clip_w, clip_h] = state.clip;
                let world_min_x = state.camera_x.saturating_add(clip_x);
                let world_max_x = world_min_x.saturating_add(clip_w.saturating_sub(1));
                let world_min_y = state.camera_y.saturating_add(clip_y);
                let world_max_y = world_min_y.saturating_add(clip_h.saturating_sub(1));
                let min_x = x0.min(x1).max(world_min_x);
                let max_x = x0.max(x1).min(world_max_x);
                let min_y = y0.min(y1).max(world_min_y);
                let max_y = y0.max(y1).min(world_max_y);
                for y in min_y..=max_y {
                    for x in min_x..=max_x {
                        draw_pixel(state, x, y, color);
                    }
                }
                state.draw_calls += 1;
                Ok(())
            },
        ),
    )?;
    set_function(
        lua,
        &globals,
        "rect",
        clone_state(
            &state,
            |state, (x0, y0, x1, y1, color): (i32, i32, i32, i32, Option<i64>)| {
                let color = color.unwrap_or(state.draw_color as i64) as u8;
                draw_line(state, x0, y0, x1, y0, color);
                draw_line(state, x1, y0, x1, y1, color);
                draw_line(state, x1, y1, x0, y1, color);
                draw_line(state, x0, y1, x0, y0, color);
                state.draw_calls += 1;
                Ok(())
            },
        ),
    )?;
    set_function(
        lua,
        &globals,
        "line",
        clone_state(
            &state,
            |state, (x0, y0, x1, y1, color): (i32, i32, i32, i32, Option<i64>)| {
                draw_line(
                    state,
                    x0,
                    y0,
                    x1,
                    y1,
                    color.unwrap_or(state.draw_color as i64) as u8,
                );
                state.draw_calls += 1;
                Ok(())
            },
        ),
    )?;
    set_function(
        lua,
        &globals,
        "color",
        clone_state(&state, |state, color: Option<i64>| {
            let previous = state.draw_color;
            if let Some(color) = color {
                state.draw_color = color as u8 & 0x0f;
            }
            Ok(previous as i64)
        }),
    )?;
    set_function(
        lua,
        &globals,
        "camera",
        clone_state(&state, |state, args: Variadic<i32>| {
            let old = (state.camera_x, state.camera_y);
            state.camera_x = args.first().copied().unwrap_or(0).clamp(-32_768, 32_767);
            state.camera_y = args.get(1).copied().unwrap_or(0).clamp(-32_768, 32_767);
            Ok(old)
        }),
    )?;
    set_function(
        lua,
        &globals,
        "clip",
        clone_state(&state, |state, args: Variadic<i32>| {
            let old = (state.clip[0], state.clip[1], state.clip[2], state.clip[3]);
            state.clip = if args.len() >= 4 {
                [
                    args[0].clamp(-32_768, 32_767),
                    args[1].clamp(-32_768, 32_767),
                    args[2].clamp(0, DISPLAY_WIDTH as i32),
                    args[3].clamp(0, DISPLAY_HEIGHT as i32),
                ]
            } else {
                [0, 0, DISPLAY_WIDTH as i32, DISPLAY_HEIGHT as i32]
            };
            Ok(old)
        }),
    )?;

    install_memory_api(lua, &globals, &state)?;
    install_asset_api(lua, &globals, &state)?;
    install_input_api(lua, &globals, &state)?;
    install_math_api(lua, &globals, &state)?;
    install_table_api(lua, &globals)?;

    set_function(lua, &globals, "flip", |_, _args: MultiValue| Ok(()))?;
    install_palette_api(lua, &globals, &state)?;
    for name in ["sfx", "music"] {
        let api_state = Arc::clone(&state);
        let function = lua
            .create_function(move |_, _args: MultiValue| {
                lock(&api_state).audio_calls += 1;
                Ok(())
            })
            .map_err(|error| error.to_string())?;
        globals
            .set(name, function)
            .map_err(|error| error.to_string())?;
    }
    let print_state = Arc::clone(&state);
    globals
        .set(
            "print",
            lua.create_function(move |_, args: Variadic<Value>| {
                let text = args
                    .iter()
                    .map(value_to_text)
                    .collect::<Vec<_>>()
                    .join("\t");
                let mut state = lock(&print_state);
                if state.printed.len() < MAX_PRINT_LOG_ENTRIES {
                    let mut text = text;
                    text.truncate(text.floor_char_boundary(MAX_PRINT_ENTRY_BYTES));
                    state.printed.push(text);
                }
                Ok(0_i64)
            })
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;

    Ok(())
}

fn install_memory_api(
    lua: &Lua,
    globals: &Table,
    state: &Arc<Mutex<RuntimeSnapshot>>,
) -> Result<(), String> {
    set_function(
        lua,
        globals,
        "peek",
        clone_state(state, |state, address: i64| {
            Ok(state.ram[wrap_address(address)] as i64)
        }),
    )?;
    set_function(
        lua,
        globals,
        "peek2",
        clone_state(state, |state, address: i64| {
            let address = wrap_address(address);
            let value =
                state.ram[address] as u16 | ((state.ram[(address + 1) & 0xffff] as u16) << 8);
            Ok(value as i16 as i64)
        }),
    )?;
    set_function(
        lua,
        globals,
        "peek4",
        clone_state(state, |state, address: i64| {
            let address = wrap_address(address);
            let mut bytes = [0_u8; 4];
            for (offset, byte) in bytes.iter_mut().enumerate() {
                *byte = state.ram[(address + offset) & 0xffff];
            }
            Ok(i32::from_le_bytes(bytes) as f64 / 65_536.0)
        }),
    )?;
    set_function(
        lua,
        globals,
        "poke",
        clone_state(state, |state, args: Variadic<i64>| {
            if let Some(address) = args.first() {
                let address = wrap_address(*address);
                for (offset, value) in args.iter().skip(1).enumerate() {
                    state.ram[(address + offset) & 0xffff] = *value as u8;
                }
            }
            Ok(())
        }),
    )?;
    set_function(
        lua,
        globals,
        "poke2",
        clone_state(state, |state, (address, value): (i64, i64)| {
            let address = wrap_address(address);
            let bytes = (value as i16).to_le_bytes();
            state.ram[address] = bytes[0];
            state.ram[(address + 1) & 0xffff] = bytes[1];
            Ok(())
        }),
    )?;
    set_function(
        lua,
        globals,
        "poke4",
        clone_state(state, |state, (address, value): (i64, f64)| {
            let address = wrap_address(address);
            let fixed = (value * 65_536.0) as i32;
            for (offset, byte) in fixed.to_le_bytes().iter().enumerate() {
                state.ram[(address + offset) & 0xffff] = *byte;
            }
            Ok(())
        }),
    )?;
    set_function(
        lua,
        globals,
        "memset",
        clone_state(
            state,
            |state, (destination, value, length): (i64, i64, i64)| {
                for offset in 0..length.clamp(0, RAM_BYTES as i64) as usize {
                    state.ram[(wrap_address(destination) + offset) & 0xffff] = value as u8;
                }
                Ok(())
            },
        ),
    )?;
    set_function(
        lua,
        globals,
        "memcpy",
        clone_state(
            state,
            |state, (destination, source, length): (i64, i64, i64)| {
                let destination = wrap_address(destination);
                let source = wrap_address(source);
                let values: Vec<u8> = (0..length.clamp(0, RAM_BYTES as i64) as usize)
                    .map(|offset| state.ram[(source + offset) & 0xffff])
                    .collect();
                for (offset, value) in values.into_iter().enumerate() {
                    state.ram[(destination + offset) & 0xffff] = value;
                }
                Ok(())
            },
        ),
    )?;
    Ok(())
}

fn install_palette_api(
    lua: &Lua,
    globals: &Table,
    state: &Arc<Mutex<RuntimeSnapshot>>,
) -> Result<(), String> {
    set_function(
        lua,
        globals,
        "pal",
        clone_state(
            state,
            |state, (source, target, palette): (Option<i64>, Option<i64>, Option<i64>)| {
                match (source, target) {
                    (Some(source), Some(target)) => {
                        let source = source as usize & 0x0f;
                        let target = target as u8 & 0x0f;
                        if palette.unwrap_or(0) == 1 {
                            state.display_palette[source] = target;
                        } else {
                            state.draw_palette[source] = target;
                        }
                    }
                    _ => {
                        state.draw_palette = std::array::from_fn(|index| index as u8);
                        state.display_palette = std::array::from_fn(|index| index as u8);
                    }
                }
                Ok(())
            },
        ),
    )?;
    set_function(
        lua,
        globals,
        "palt",
        clone_state(
            state,
            |state, (color, transparent): (Option<i64>, Option<bool>)| {
                if let Some(color) = color {
                    state.transparent[color as usize & 0x0f] = transparent.unwrap_or(true);
                } else {
                    state.transparent = std::array::from_fn(|index| index == 0);
                }
                Ok(())
            },
        ),
    )?;
    Ok(())
}

fn install_asset_api(
    lua: &Lua,
    globals: &Table,
    state: &Arc<Mutex<RuntimeSnapshot>>,
) -> Result<(), String> {
    set_function(
        lua,
        globals,
        "sget",
        clone_state(state, |state, (x, y): (i32, i32)| {
            Ok(get_nibble_pixel(&state.ram, 0, x, y, 128, 128) as i64)
        }),
    )?;
    set_function(
        lua,
        globals,
        "sset",
        clone_state(state, |state, (x, y, color): (i32, i32, i64)| {
            set_nibble_pixel(&mut state.ram, 0, x, y, 128, 128, color as u8);
            Ok(())
        }),
    )?;
    set_function(
        lua,
        globals,
        "mget",
        clone_state(state, |state, (x, y): (i32, i32)| {
            if !(0..128).contains(&x) || !(0..64).contains(&y) {
                return Ok(0_i64);
            }
            let address = if y < 32 {
                0x2000 + y as usize * 128 + x as usize
            } else {
                0x1000 + (y as usize - 32) * 128 + x as usize
            };
            Ok(state.ram[address] as i64)
        }),
    )?;
    set_function(
        lua,
        globals,
        "mset",
        clone_state(state, |state, (x, y, value): (i32, i32, i64)| {
            if (0..128).contains(&x) && (0..64).contains(&y) {
                let address = if y < 32 {
                    0x2000 + y as usize * 128 + x as usize
                } else {
                    0x1000 + (y as usize - 32) * 128 + x as usize
                };
                state.ram[address] = value as u8;
            }
            Ok(())
        }),
    )?;
    set_function(
        lua,
        globals,
        "fget",
        clone_state(state, |state, (sprite, flag): (i64, Option<i64>)| {
            let value = state.ram[0x3000 + (sprite as usize & 0xff)];
            match flag {
                Some(flag) => Ok(ValueResult::Bool(value & (1 << (flag as u8 & 7)) != 0)),
                None => Ok(ValueResult::Int(value as i64)),
            }
        }),
    )?;
    set_function(
        lua,
        globals,
        "fset",
        clone_state(
            state,
            |state, (sprite, flag, enabled): (i64, i64, Option<bool>)| {
                let address = 0x3000 + (sprite as usize & 0xff);
                if let Some(enabled) = enabled {
                    let bit = 1 << (flag as u8 & 7);
                    if enabled {
                        state.ram[address] |= bit;
                    } else {
                        state.ram[address] &= !bit;
                    }
                } else {
                    state.ram[address] = flag as u8;
                }
                Ok(())
            },
        ),
    )?;
    set_function(
        lua,
        globals,
        "spr",
        clone_state(
            state,
            |state, (sprite, x, y, width, height, flip_x, flip_y): SprArgs| {
                let sprite = sprite as i32;
                let tiles_w = width.unwrap_or(1).clamp(0, 256);
                let tiles_h = height.unwrap_or(1).clamp(0, 256);
                blit_sprite(
                    state,
                    (sprite & 0xf) * 8,
                    (sprite >> 4) * 8,
                    tiles_w * 8,
                    tiles_h * 8,
                    x,
                    y,
                    tiles_w * 8,
                    tiles_h * 8,
                    flip_x.unwrap_or(false),
                    flip_y.unwrap_or(false),
                );
                state.draw_calls += 1;
                Ok(())
            },
        ),
    )?;
    set_function(
        lua,
        globals,
        "sspr",
        clone_state(
            state,
            |state, (sx, sy, sw, sh, dx, dy, dw, dh, flip_x, flip_y): SsprArgs| {
                blit_sprite(
                    state,
                    sx,
                    sy,
                    sw,
                    sh,
                    dx,
                    dy,
                    dw.unwrap_or(sw),
                    dh.unwrap_or(sh),
                    flip_x.unwrap_or(false),
                    flip_y.unwrap_or(false),
                );
                state.draw_calls += 1;
                Ok(())
            },
        ),
    )?;
    Ok(())
}

fn install_input_api(
    lua: &Lua,
    globals: &Table,
    state: &Arc<Mutex<RuntimeSnapshot>>,
) -> Result<(), String> {
    set_function(
        lua,
        globals,
        "btn",
        clone_state(
            state,
            |state, (button, player): (Option<i64>, Option<i64>)| {
                Ok(match button {
                    Some(button) => ValueResult::Bool(
                        controller_button_bit(button, player)
                            .is_some_and(|bit| state.input_mask & bit != 0),
                    ),
                    None => ValueResult::Int(state.input_mask as i64),
                })
            },
        ),
    )?;
    set_function(
        lua,
        globals,
        "btnp",
        clone_state(
            state,
            |state, (button, player): (Option<i64>, Option<i64>)| {
                let pressed = state.input_mask & !state.previous_input_mask;
                Ok(match button {
                    Some(button) => ValueResult::Bool(
                        controller_button_bit(button, player).is_some_and(|bit| pressed & bit != 0),
                    ),
                    None => ValueResult::Int(pressed as i64),
                })
            },
        ),
    )?;
    set_function(
        lua,
        globals,
        "time",
        clone_state(state, |state, (): ()| {
            Ok(state.frame as f64 / f64::from(state.callback_hz))
        }),
    )?;
    set_function(
        lua,
        globals,
        "t",
        clone_state(state, |state, (): ()| {
            Ok(state.frame as f64 / f64::from(state.callback_hz))
        }),
    )?;
    set_function(
        lua,
        globals,
        "stat",
        clone_state(state, |state, index: i64| {
            Ok(match index {
                1 => state.frame as f64 / f64::from(state.callback_hz),
                7 => f64::from(state.callback_hz),
                _ => 0.0,
            })
        }),
    )?;
    Ok(())
}

fn controller_button_bit(button: i64, player: Option<i64>) -> Option<u16> {
    let button = u32::try_from(button).ok().filter(|button| *button < 6)?;
    let player = u32::try_from(player.unwrap_or(0))
        .ok()
        .filter(|player| *player < 2)?;
    Some(1_u16 << (player * 8 + button))
}

fn install_math_api(
    lua: &Lua,
    globals: &Table,
    state: &Arc<Mutex<RuntimeSnapshot>>,
) -> Result<(), String> {
    set_function(
        lua,
        globals,
        "srand",
        clone_state(state, |state, seed: Option<f64>| {
            state.rng_state = seed.unwrap_or(0.0).to_bits() ^ 0xa076_1d64_78bd_642f;
            Ok(())
        }),
    )?;
    set_function(
        lua,
        globals,
        "rnd",
        clone_state(state, |state, upper: Option<f64>| {
            state.rng_state = state
                .rng_state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let unit = ((state.rng_state >> 11) as f64) / ((1_u64 << 53) as f64);
            Ok(unit * upper.unwrap_or(1.0))
        }),
    )?;
    set_function(lua, globals, "flr", |_, value: f64| Ok(value.floor()))?;
    set_function(lua, globals, "ceil", |_, value: f64| Ok(value.ceil()))?;
    set_function(lua, globals, "sqrt", |_, value: f64| Ok(value.sqrt()))?;
    set_function(lua, globals, "abs", |_, value: f64| Ok(value.abs()))?;
    set_function(lua, globals, "min", |_, (left, right): (f64, f64)| {
        Ok(left.min(right))
    })?;
    set_function(lua, globals, "max", |_, (left, right): (f64, f64)| {
        Ok(left.max(right))
    })?;
    set_function(lua, globals, "sgn", |_, value: f64| {
        Ok(if value < 0.0 { -1.0 } else { 1.0 })
    })?;
    set_function(
        lua,
        globals,
        "sin",
        |_, value: f64| Ok(-(value * TAU).sin()),
    )?;
    set_function(lua, globals, "cos", |_, value: f64| Ok((value * TAU).cos()))?;
    set_function(lua, globals, "atan2", |_, (dx, dy): (f64, f64)| {
        Ok((-dx.atan2(dy) / TAU).rem_euclid(1.0))
    })?;
    set_function(lua, globals, "mid", |_, (a, b, c): (f64, f64, f64)| {
        let mut values = [a, b, c];
        values.sort_by(f64::total_cmp);
        Ok(values[1])
    })?;
    set_function(lua, globals, "band", |_, values: Variadic<i64>| {
        Ok(values
            .iter()
            .fold(-1_i32, |result, value| result & *value as i32) as i64)
    })?;
    set_function(lua, globals, "bor", |_, values: Variadic<i64>| {
        Ok(values
            .iter()
            .fold(0_i32, |result, value| result | *value as i32) as i64)
    })?;
    set_function(lua, globals, "bxor", |_, values: Variadic<i64>| {
        Ok(values
            .iter()
            .fold(0_i32, |result, value| result ^ *value as i32) as i64)
    })?;
    set_function(lua, globals, "bnot", |_, value: i64| {
        Ok((!(value as i32)) as i64)
    })?;
    set_function(lua, globals, "shl", |_, (value, count): (i64, i64)| {
        Ok((value as i32).wrapping_shl(count as u32 & 31) as i64)
    })?;
    set_function(lua, globals, "shr", |_, (value, count): (i64, i64)| {
        Ok(((value as i32) >> (count as u32 & 31)) as i64)
    })?;
    set_function(lua, globals, "lshr", |_, (value, count): (i64, i64)| {
        Ok(((value as u32) >> (count as u32 & 31)) as i64)
    })?;
    set_function(lua, globals, "rotl", |_, (value, count): (i64, i64)| {
        Ok((value as u32).rotate_left(count as u32 & 31) as i32 as i64)
    })?;
    set_function(lua, globals, "rotr", |_, (value, count): (i64, i64)| {
        Ok((value as u32).rotate_right(count as u32 & 31) as i32 as i64)
    })?;
    Ok(())
}

fn install_table_api(lua: &Lua, globals: &Table) -> Result<(), String> {
    set_function(
        lua,
        globals,
        "add",
        |_, (table, value, index): (Table, Value, Option<i64>)| {
            let index = index.unwrap_or(table.raw_len() as i64 + 1).max(1);
            for cursor in (index as usize..=table.raw_len()).rev() {
                let existing: Value = table.raw_get(cursor)?;
                table.raw_set(cursor + 1, existing)?;
            }
            table.raw_set(index, value.clone())?;
            Ok(value)
        },
    )?;
    set_function(
        lua,
        globals,
        "deli",
        |_, (table, index): (Table, Option<i64>)| {
            let length = table.raw_len();
            let index = index.unwrap_or(length as i64).max(1) as usize;
            if index > length {
                return Ok(Value::Nil);
            }
            let removed: Value = table.raw_get(index)?;
            for cursor in index..length {
                let next: Value = table.raw_get(cursor + 1)?;
                table.raw_set(cursor, next)?;
            }
            table.raw_set(length, Value::Nil)?;
            Ok(removed)
        },
    )?;
    set_function(lua, globals, "count", |_, table: Table| {
        Ok(table.raw_len() as i64)
    })?;
    Ok(())
}

fn set_function<A, R, F>(lua: &Lua, globals: &Table, name: &str, function: F) -> Result<(), String>
where
    A: mlua::FromLuaMulti,
    R: mlua::IntoLuaMulti,
    F: Fn(&Lua, A) -> mlua::Result<R> + Send + 'static,
{
    let function = lua
        .create_function(function)
        .map_err(|error| format!("cannot create PICO-8 API {name}: {error}"))?;
    globals
        .set(name, function)
        .map_err(|error| format!("cannot register PICO-8 API {name}: {error}"))
}

fn clone_state<A, R, F>(
    state: &Arc<Mutex<RuntimeSnapshot>>,
    function: F,
) -> impl Fn(&Lua, A) -> mlua::Result<R> + Send + 'static
where
    F: Fn(&mut RuntimeSnapshot, A) -> mlua::Result<R> + Send + 'static,
{
    let state = Arc::clone(state);
    move |_lua, args| function(&mut lock(&state), args)
}

#[derive(Debug)]
enum ValueResult {
    Bool(bool),
    Int(i64),
}

impl mlua::IntoLua for ValueResult {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        match self {
            Self::Bool(value) => value.into_lua(lua),
            Self::Int(value) => value.into_lua(lua),
        }
    }
}

fn wrap_address(address: i64) -> usize {
    address as usize & 0xffff
}

fn draw_pixel(state: &mut RuntimeSnapshot, x: i32, y: i32, color: u8) {
    let x = x.saturating_sub(state.camera_x);
    let y = y.saturating_sub(state.camera_y);
    let [clip_x, clip_y, clip_w, clip_h] = state.clip;
    if x >= clip_x && y >= clip_y && x < clip_x + clip_w && y < clip_y + clip_h {
        set_pixel(
            &mut state.ram,
            x,
            y,
            state.draw_palette[color as usize & 0x0f],
        );
    }
}

fn draw_line(
    state: &mut RuntimeSnapshot,
    mut x0: i32,
    mut y0: i32,
    mut x1: i32,
    mut y1: i32,
    color: u8,
) {
    x0 = x0.clamp(-65_536, 65_536);
    y0 = y0.clamp(-65_536, 65_536);
    x1 = x1.clamp(-65_536, 65_536);
    y1 = y1.clamp(-65_536, 65_536);
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    for _ in 0..=262_144 {
        draw_pixel(state, x0, y0, color);
        if x0 == x1 && y0 == y1 {
            return;
        }
        let twice = 2 * error;
        if twice >= dy {
            error += dy;
            x0 += sx;
        }
        if twice <= dx {
            error += dx;
            y0 += sy;
        }
    }
}

fn get_nibble_pixel(ram: &[u8], start: usize, x: i32, y: i32, width: i32, height: i32) -> u8 {
    if !(0..width).contains(&x) || !(0..height).contains(&y) {
        return 0;
    }
    let address = start + y as usize * width as usize / 2 + x as usize / 2;
    if x & 1 == 0 {
        ram[address] & 0x0f
    } else {
        ram[address] >> 4
    }
}

fn set_nibble_pixel(
    ram: &mut [u8],
    start: usize,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    color: u8,
) {
    if !(0..width).contains(&x) || !(0..height).contains(&y) {
        return;
    }
    let address = start + y as usize * width as usize / 2 + x as usize / 2;
    if x & 1 == 0 {
        ram[address] = (ram[address] & 0xf0) | (color & 0x0f);
    } else {
        ram[address] = (ram[address] & 0x0f) | ((color & 0x0f) << 4);
    }
}

#[allow(clippy::too_many_arguments)]
fn blit_sprite(
    state: &mut RuntimeSnapshot,
    sx: i32,
    sy: i32,
    sw: i32,
    sh: i32,
    dx: i32,
    dy: i32,
    dw: i32,
    dh: i32,
    flip_x: bool,
    flip_y: bool,
) {
    if sw <= 0 || sh <= 0 || dw <= 0 || dh <= 0 {
        return;
    }
    let [clip_x, clip_y, clip_w, clip_h] = state.clip;
    let visible_min_x = state.camera_x.saturating_add(clip_x);
    let visible_max_x = visible_min_x.saturating_add(clip_w.saturating_sub(1));
    let visible_min_y = state.camera_y.saturating_add(clip_y);
    let visible_max_y = visible_min_y.saturating_add(clip_h.saturating_sub(1));
    let start_x = visible_min_x.saturating_sub(dx).clamp(0, dw);
    let end_x = visible_max_x
        .saturating_sub(dx)
        .saturating_add(1)
        .clamp(0, dw);
    let start_y = visible_min_y.saturating_sub(dy).clamp(0, dh);
    let end_y = visible_max_y
        .saturating_sub(dy)
        .saturating_add(1)
        .clamp(0, dh);
    for output_y in start_y..end_y {
        for output_x in start_x..end_x {
            let source_x = output_x * sw / dw;
            let source_y = output_y * sh / dh;
            let source_x = if flip_x { sw - 1 - source_x } else { source_x };
            let source_y = if flip_y { sh - 1 - source_y } else { source_y };
            let color = get_nibble_pixel(&state.ram, 0, sx + source_x, sy + source_y, 128, 128);
            if !state.transparent[color as usize & 0x0f] {
                draw_pixel(state, dx + output_x, dy + output_y, color);
            }
        }
    }
}

fn value_to_text(value: &Value) -> String {
    match value {
        Value::Nil => "[nil]".into(),
        Value::Boolean(value) => value.to_string(),
        Value::Integer(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.to_string_lossy(),
        other => format!("[{}]", other.type_name()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cart(lua: &str) -> Cartridge {
        let source =
            format!("pico-8 cartridge // http://www.pico-8.com\nversion 42\n__lua__\n{lua}\n");
        Cartridge::parse(source.as_bytes()).expect("cart")
    }

    #[test]
    fn executes_callbacks_and_draws_into_screen_memory() {
        let runtime = Pico8Runtime::new(
            &cart("x=0\nfunction _update60() x+=1 end\nfunction _draw() pset(x,2,9) end"),
            7,
            100_000,
        )
        .expect("runtime");
        assert_eq!(runtime.fps(), 60);
        runtime.step_frame().expect("frame");
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.frame, 1);
        assert_eq!(get_pixel(&snapshot.ram, 1, 2), 9);
    }

    #[test]
    fn input_and_seeded_randomness_are_deterministic() {
        let source =
            "function _update() if btnp(4) then pset(0,0,7) end pset(1,0,flr(rnd(16))) end";
        let left = Pico8Runtime::new(&cart(source), 99, 100_000).expect("left");
        let right = Pico8Runtime::new(&cart(source), 99, 100_000).expect("right");
        left.set_input_mask(1 << 4);
        right.set_input_mask(1 << 4);
        left.step_frame().expect("left frame");
        right.step_frame().expect("right frame");
        assert_eq!(left.snapshot(), right.snapshot());
        assert_eq!(get_pixel(&left.snapshot().ram, 0, 0), 7);
    }

    #[test]
    fn instruction_hook_terminates_runaway_cart() {
        let error = Pico8Runtime::new(&cart("while true do end"), 0, 10_000)
            .err()
            .expect("runaway cart should fail");
        assert!(error.contains("instruction budget"));
    }

    #[test]
    fn snapshot_has_documented_ram_size() {
        let runtime = Pico8Runtime::new(&cart(""), 0, 100_000).expect("runtime");
        assert_eq!(runtime.snapshot().ram.len(), RAM_BYTES);
    }

    #[test]
    fn runtime_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Pico8Runtime>();
    }

    #[test]
    fn update60_time_and_stat_use_sixty_hertz() {
        let runtime = Pico8Runtime::new(
            &cart("function _update60() pset(flr(time()*60),0,7) pset(flr(stat(1)*60),1,8) end"),
            0,
            100_000,
        )
        .expect("runtime");
        runtime.step_frame().expect("frame 0");
        runtime.step_frame().expect("frame 1");
        let state = runtime.snapshot();
        assert_eq!(state.callback_hz, 60);
        assert_eq!(get_pixel(&state.ram, 1, 0), 7);
        assert_eq!(get_pixel(&state.ram, 1, 1), 8);
    }

    #[test]
    fn sandbox_removes_host_access_alternate_rng_and_catchable_budget_paths() {
        Pico8Runtime::new(
            &cart(
                "assert(io==nil and os==nil and package==nil and require==nil)\nassert(math==nil and coroutine==nil and pcall==nil and xpcall==nil)",
            ),
            0,
            100_000,
        )
        .expect("sandbox");
    }

    #[test]
    fn huge_native_ranges_are_bounded_to_machine_memory_and_display() {
        let runtime = Pico8Runtime::new(
            &cart(
                "function _draw() rectfill(-0x7fff,-0x7fff,0x7fff,0x7fff,4) memset(0,3,0x7fffffff) end",
            ),
            0,
            100_000,
        )
        .expect("runtime");
        runtime.step_frame().expect("bounded frame");
        let state = runtime.snapshot();
        assert_eq!(state.ram.len(), RAM_BYTES);
        assert_eq!(get_pixel(&state.ram, 126, 127), 3);
    }

    #[test]
    fn palette_and_transparency_affect_sprite_blits_deterministically() {
        let runtime = Pico8Runtime::new(
            &cart("sset(0,0,2)\nfunction _draw() pal(2,9) spr(0,0,0) palt(2,true) spr(0,1,0) end"),
            0,
            100_000,
        )
        .expect("runtime");
        runtime.step_frame().expect("frame");
        let state = runtime.snapshot();
        assert_eq!(get_pixel(&state.ram, 0, 0), 9);
        assert_eq!(get_pixel(&state.ram, 1, 0), 0);
    }
}
