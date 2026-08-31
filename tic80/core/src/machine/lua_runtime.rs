use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use mlua::{
    Function, HookTriggers, Lua, LuaOptions, MultiValue, StdLib, Value as LuaValue, VmState,
};

use crate::configuration::{
    FLAGS_ADDR, GAMEPAD_ADDR, HEIGHT, LUA_HOOK_GRANULARITY, LUA_INSTRUCTION_BUDGET,
    LUA_MEMORY_LIMIT_BYTES, MAP_ADDR, PALETTE_ADDR, RAM_BYTES, SCREEN_BYTES, TILES_ADDR,
    VRAM_BYTES, WIDTH,
};
use crate::event::Tic80Event;
use crate::execution::FrameOutcome;
use crate::output::Framebuffer;
use crate::snapshot::RuntimeSnapshot;

use super::SWEETIE_16;
use super::cartridge::{ParsedCart, parse_cart};

type SpriteArgs = (
    usize,
    i32,
    i32,
    Option<LuaValue>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
);
type MapArgs = (
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<LuaValue>,
    Option<i32>,
);
type PrintArgs = (
    String,
    Option<i32>,
    Option<i32>,
    Option<u8>,
    Option<bool>,
    Option<i32>,
    Option<bool>,
);

struct RuntimeState {
    ram: Vec<u8>,
    vram1: Vec<u8>,
    vbank: u8,
    frame: u64,
    input: u32,
    previous_input: u32,
    holds: [u32; 32],
    clip: [i32; 4],
    traces: Vec<String>,
    exit_requested: bool,
}

impl RuntimeState {
    fn from_cart(cart: &ParsedCart) -> Self {
        Self {
            ram: cart.initial_ram.clone(),
            vram1: vec![0; VRAM_BYTES],
            vbank: 0,
            frame: 0,
            input: 0,
            previous_input: 0,
            holds: [0; 32],
            clip: [0, 0, WIDTH as i32, HEIGHT as i32],
            traces: Vec::new(),
            exit_requested: false,
        }
    }

    fn active_vram(&self) -> &[u8] {
        if self.vbank == 0 {
            &self.ram[..VRAM_BYTES]
        } else {
            &self.vram1
        }
    }

    fn active_vram_mut(&mut self) -> &mut [u8] {
        if self.vbank == 0 {
            &mut self.ram[..VRAM_BYTES]
        } else {
            &mut self.vram1
        }
    }

    fn get_pixel(&self, x: i32, y: i32) -> u8 {
        if x < 0 || y < 0 || x >= WIDTH as i32 || y >= HEIGHT as i32 {
            return 0;
        }
        let pixel = y as usize * WIDTH + x as usize;
        let byte = self.active_vram()[pixel / 2];
        if pixel & 1 == 0 {
            byte & 0x0f
        } else {
            byte >> 4
        }
    }

    fn set_pixel(&mut self, x: i32, y: i32, color: u8) {
        if x < self.clip[0]
            || y < self.clip[1]
            || x >= self.clip[0] + self.clip[2]
            || y >= self.clip[1] + self.clip[3]
            || x < 0
            || y < 0
            || x >= WIDTH as i32
            || y >= HEIGHT as i32
        {
            return;
        }
        let pixel = y as usize * WIDTH + x as usize;
        let byte = &mut self.active_vram_mut()[pixel / 2];
        if pixel & 1 == 0 {
            *byte = (*byte & 0xf0) | (color & 0x0f);
        } else {
            *byte = (*byte & 0x0f) | ((color & 0x0f) << 4);
        }
    }

    fn set_input(&mut self, mask: u32) {
        self.previous_input = self.input;
        self.input = mask;
        self.ram[GAMEPAD_ADDR..GAMEPAD_ADDR + 4].copy_from_slice(&mask.to_le_bytes());
        for (button, hold) in self.holds.iter_mut().enumerate() {
            if mask & (1 << button) != 0 {
                *hold = hold.saturating_add(1);
            } else {
                *hold = 0;
            }
        }
    }
}

pub struct Tic80Runtime {
    cart_bytes: Vec<u8>,
    seed: u64,
    lua: Lua,
    shared: Arc<Mutex<RuntimeState>>,
    input_history: Vec<u32>,
    reported_trace_count: usize,
    instructions_remaining: Arc<AtomicU64>,
}

fn lua_state(state: &Arc<Mutex<RuntimeState>>) -> mlua::Result<MutexGuard<'_, RuntimeState>> {
    state
        .lock()
        .map_err(|_| mlua::Error::RuntimeError("TIC-80 state lock poisoned".into()))
}

impl Tic80Runtime {
    pub fn new(cart_bytes: &[u8], seed: u64) -> Result<Self, String> {
        let cart = parse_cart(cart_bytes)?;
        if cart.language != "lua" {
            return Err(format!(
                "unsupported TIC-80 cartridge language {:?}; this bundle executes Lua carts through its bounded Lua 5.4 compatibility runtime",
                cart.language
            ));
        }
        let shared = Arc::new(Mutex::new(RuntimeState::from_cart(&cart)));
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH,
            LuaOptions::new().catch_rust_panics(false),
        )
        .map_err(|error| format!("cannot construct bounded TIC-80 Lua state: {error}"))?;
        lua.set_memory_limit(LUA_MEMORY_LIMIT_BYTES)
            .map_err(|error| format!("cannot set TIC-80 Lua memory limit: {error}"))?;
        install_lua_sandbox(&lua)?;
        install_lua_api(&lua, shared.clone()).map_err(|error| error.to_string())?;

        let instructions_remaining = Arc::new(AtomicU64::new(LUA_INSTRUCTION_BUDGET));
        let hook_budget = Arc::clone(&instructions_remaining);
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(LUA_HOOK_GRANULARITY as u32),
            move |_lua, _debug| {
                let previous =
                    hook_budget.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                        remaining.checked_sub(LUA_HOOK_GRANULARITY)
                    });
                if previous.is_err() {
                    return Err(mlua::Error::RuntimeError(
                        "TIC-80 instruction budget exhausted".into(),
                    ));
                }
                Ok(VmState::Continue)
            },
        );

        instructions_remaining.store(LUA_INSTRUCTION_BUDGET, Ordering::Relaxed);
        lua.load(&cart.code)
            .set_name("cartridge")
            .exec()
            .map_err(|error| format!("TIC-80 Lua load failed: {error}"))?;
        if let Ok(randomseed) = lua.globals().get::<Function>("math.randomseed") {
            randomseed
                .call::<()>((seed as i64,))
                .map_err(|error| format!("TIC-80 RNG initialization failed: {error}"))?;
        }
        if let Ok(boot) = lua.globals().get::<Function>("BOOT") {
            instructions_remaining.store(LUA_INSTRUCTION_BUDGET, Ordering::Relaxed);
            boot.call::<()>(())
                .map_err(|error| format!("TIC-80 BOOT failed: {error}"))?;
        }
        if lua.globals().get::<Function>("TIC").is_err() {
            return Err("TIC-80 Lua cartridge does not define TIC()".into());
        }
        Ok(Self {
            cart_bytes: cart_bytes.to_vec(),
            seed,
            lua,
            shared,
            input_history: Vec::new(),
            reported_trace_count: 0,
            instructions_remaining,
        })
    }

    pub fn tick(&mut self, input: u32) -> Result<FrameOutcome, String> {
        {
            let mut state = self
                .shared
                .lock()
                .map_err(|_| "TIC-80 state lock poisoned")?;
            state.set_input(input);
        }
        let tic = self
            .lua
            .globals()
            .get::<Function>("TIC")
            .map_err(|error| format!("TIC-80 TIC callback missing: {error}"))?;
        self.instructions_remaining
            .store(LUA_INSTRUCTION_BUDGET, Ordering::Relaxed);
        tic.call::<()>(())
            .map_err(|error| format!("TIC-80 TIC callback failed: {error}"))?;
        let mut state = self
            .shared
            .lock()
            .map_err(|_| "TIC-80 state lock poisoned")?;
        state.frame += 1;
        let frame = state.frame;
        let exit_requested = state.exit_requested;
        let traces = state.traces[self.reported_trace_count..].to_vec();
        let reported_trace_count = state.traces.len();
        drop(state);
        self.reported_trace_count = reported_trace_count;
        self.input_history.push(input);
        let mut events = Vec::with_capacity(traces.len() + 2);
        events.push(Tic80Event::InputSampled { mask: input });
        events.extend(
            traces
                .into_iter()
                .map(|message| Tic80Event::Trace { message }),
        );
        events.push(Tic80Event::FrameCompleted { frame });
        Ok(FrameOutcome {
            frame,
            exit_requested,
            events,
        })
    }

    pub fn reset(&mut self) -> Result<(), String> {
        *self = Self::new(&self.cart_bytes, self.seed)?;
        Ok(())
    }

    pub fn restore_history(&mut self, history: &[u32]) -> Result<(), String> {
        let mut candidate = Self::new(&self.cart_bytes, self.seed)?;
        for input in history {
            candidate.tick(*input)?;
        }
        *self = candidate;
        Ok(())
    }

    pub fn artifact_bytes(&self) -> &[u8] {
        &self.cart_bytes
    }

    pub fn input_history(&self) -> &[u32] {
        &self.input_history
    }

    pub fn snapshot(&self) -> Result<RuntimeSnapshot, String> {
        let state = self
            .shared
            .lock()
            .map_err(|_| "TIC-80 state lock poisoned")?;
        Ok(RuntimeSnapshot {
            ram: state.ram.clone(),
            overlay_vram: state.vram1.clone(),
            active_video_bank: state.vbank,
            frame: state.frame,
            input: state.input,
            previous_input: state.previous_input,
            button_holds: state.holds,
            clip: state.clip,
            traces: state.traces.clone(),
            exit_requested: state.exit_requested,
            input_history: self.input_history.clone(),
        })
    }

    pub fn restore_snapshot(&mut self, expected: &RuntimeSnapshot) -> Result<(), String> {
        let mut candidate = Self::new(&self.cart_bytes, self.seed)?;
        candidate.restore_history(&expected.input_history)?;
        if candidate.snapshot()? != *expected {
            return Err("TIC-80 snapshot does not match deterministic replay".into());
        }
        *self = candidate;
        Ok(())
    }

    /// Restore the machine-visible state directly. This is intentionally
    /// separate from `restore_snapshot`, whose legacy native test path uses
    /// input history to reconstruct hidden Lua state. GlassVM session
    /// snapshots use this state-only seam and never persist that history.
    pub fn restore_machine_state(&mut self, expected: &RuntimeSnapshot) -> Result<(), String> {
        if expected.ram.len() != RAM_BYTES {
            return Err(format!(
                "TIC-80 machine state has {} RAM bytes; expected {RAM_BYTES}",
                expected.ram.len()
            ));
        }
        if expected.overlay_vram.len() != VRAM_BYTES {
            return Err(format!(
                "TIC-80 machine state has {} overlay-VRAM bytes; expected {VRAM_BYTES}",
                expected.overlay_vram.len()
            ));
        }
        let mut state = self
            .shared
            .lock()
            .map_err(|_| "TIC-80 state lock poisoned")?;
        state.ram = expected.ram.clone();
        state.vram1 = expected.overlay_vram.clone();
        state.vbank = expected.active_video_bank;
        state.frame = expected.frame;
        state.input = expected.input;
        state.previous_input = expected.previous_input;
        state.holds = expected.button_holds;
        state.clip = expected.clip;
        state.traces.clear();
        state.exit_requested = expected.exit_requested;
        drop(state);
        self.input_history.clear();
        self.reported_trace_count = 0;
        Ok(())
    }

    /// Reconstruct hidden Lua state from caller-supplied scheduled inputs,
    /// then discard the temporary replay history. The replay inputs are
    /// continuation material supplied by the session; they are not persisted
    /// as part of the machine snapshot.
    pub fn restore_machine_state_from_inputs(
        &mut self,
        expected: &RuntimeSnapshot,
        inputs: &[u32],
    ) -> Result<(), String> {
        let mut candidate = Self::new(&self.cart_bytes, self.seed)?;
        for input in inputs {
            candidate.tick(*input)?;
        }
        let actual = candidate.snapshot()?;
        if !same_machine_state(&actual, expected) {
            return Err(
                "TIC-80 session snapshot cannot reconstruct hidden machine state from the prepared input schedule"
                    .into(),
            );
        }
        {
            let mut state = candidate
                .shared
                .lock()
                .map_err(|_| "TIC-80 state lock poisoned")?;
            state.traces.clear();
        }
        candidate.input_history.clear();
        candidate.reported_trace_count = 0;
        *self = candidate;
        Ok(())
    }

    pub fn framebuffer(&self) -> Result<Framebuffer, String> {
        let state = self
            .shared
            .lock()
            .map_err(|_| "TIC-80 state lock poisoned")?;
        Ok(Framebuffer {
            width: WIDTH,
            height: HEIGHT,
            rgba: framebuffer_rgba(&state),
        })
    }
}

fn same_machine_state(left: &RuntimeSnapshot, right: &RuntimeSnapshot) -> bool {
    left.ram == right.ram
        && left.overlay_vram == right.overlay_vram
        && left.active_video_bank == right.active_video_bank
        && left.frame == right.frame
        && left.input == right.input
        && left.previous_input == right.previous_input
        && left.button_holds == right.button_holds
        && left.clip == right.clip
        && left.exit_requested == right.exit_requested
}

fn install_lua_sandbox(lua: &Lua) -> Result<(), String> {
    let globals = lua.globals();
    for name in [
        "collectgarbage",
        "dofile",
        "load",
        "loadfile",
        "pcall",
        "require",
        "xpcall",
    ] {
        globals
            .set(name, LuaValue::Nil)
            .map_err(|error| format!("cannot sandbox TIC-80 Lua global {name}: {error}"))?;
    }
    Ok(())
}

fn install_lua_api(lua: &Lua, shared: Arc<Mutex<RuntimeState>>) -> mlua::Result<()> {
    let globals = lua.globals();

    let state = shared.clone();
    globals.set(
        "cls",
        lua.create_function(move |_, color: Option<u8>| {
            let color = color.unwrap_or(0) & 0x0f;
            let fill = color | (color << 4);
            lua_state(&state)?.active_vram_mut()[..SCREEN_BYTES].fill(fill);
            Ok(())
        })?,
    )?;

    let state = shared.clone();
    globals.set(
        "pix",
        lua.create_function(move |_, (x, y, color): (i32, i32, Option<u8>)| {
            let mut state = lua_state(&state)?;
            if let Some(color) = color {
                state.set_pixel(x, y, color);
                Ok(color)
            } else {
                Ok(state.get_pixel(x, y))
            }
        })?,
    )?;

    let state = shared.clone();
    globals.set(
        "line",
        lua.create_function(
            move |_, (x0, y0, x1, y1, color): (i32, i32, i32, i32, u8)| {
                draw_line(&mut *lua_state(&state)?, x0, y0, x1, y1, color);
                Ok(())
            },
        )?,
    )?;

    let state = shared.clone();
    globals.set(
        "rect",
        lua.create_function(move |_, (x, y, w, h, color): (i32, i32, i32, i32, u8)| {
            let mut state = lua_state(&state)?;
            for py in y..y.saturating_add(h.max(0)) {
                for px in x..x.saturating_add(w.max(0)) {
                    state.set_pixel(px, py, color);
                }
            }
            Ok(())
        })?,
    )?;

    let state = shared.clone();
    globals.set(
        "rectb",
        lua.create_function(move |_, (x, y, w, h, color): (i32, i32, i32, i32, u8)| {
            let mut state = lua_state(&state)?;
            draw_line(&mut state, x, y, x + w - 1, y, color);
            draw_line(&mut state, x, y + h - 1, x + w - 1, y + h - 1, color);
            draw_line(&mut state, x, y, x, y + h - 1, color);
            draw_line(&mut state, x + w - 1, y, x + w - 1, y + h - 1, color);
            Ok(())
        })?,
    )?;

    let state = shared.clone();
    globals.set(
        "circ",
        lua.create_function(move |_, (cx, cy, radius, color): (i32, i32, i32, u8)| {
            let mut state = lua_state(&state)?;
            let r2 = radius.max(0) * radius.max(0);
            for y in -radius.max(0)..=radius.max(0) {
                for x in -radius.max(0)..=radius.max(0) {
                    if x * x + y * y <= r2 {
                        state.set_pixel(cx + x, cy + y, color);
                    }
                }
            }
            Ok(())
        })?,
    )?;

    let state = shared.clone();
    globals.set(
        "circb",
        lua.create_function(move |_, (cx, cy, radius, color): (i32, i32, i32, u8)| {
            let mut state = lua_state(&state)?;
            let mut x = radius.max(0);
            let mut y = 0;
            let mut error = 1 - x;
            while x >= y {
                for (px, py) in [
                    (x, y),
                    (y, x),
                    (-y, x),
                    (-x, y),
                    (-x, -y),
                    (-y, -x),
                    (y, -x),
                    (x, -y),
                ] {
                    state.set_pixel(cx + px, cy + py, color);
                }
                y += 1;
                if error < 0 {
                    error += 2 * y + 1;
                } else {
                    x -= 1;
                    error += 2 * (y - x) + 1;
                }
            }
            Ok(())
        })?,
    )?;

    let state = shared.clone();
    globals.set(
        "tri",
        lua.create_function(
            move |_, (x1, y1, x2, y2, x3, y3, color): (i32, i32, i32, i32, i32, i32, u8)| {
                fill_triangle(
                    &mut *lua_state(&state)?,
                    [(x1, y1), (x2, y2), (x3, y3)],
                    color,
                );
                Ok(())
            },
        )?,
    )?;
    let state = shared.clone();
    globals.set(
        "trib",
        lua.create_function(
            move |_, (x1, y1, x2, y2, x3, y3, color): (i32, i32, i32, i32, i32, i32, u8)| {
                let mut state = lua_state(&state)?;
                draw_line(&mut state, x1, y1, x2, y2, color);
                draw_line(&mut state, x2, y2, x3, y3, color);
                draw_line(&mut state, x3, y3, x1, y1, color);
                Ok(())
            },
        )?,
    )?;

    let state = shared.clone();
    globals.set(
        "btn",
        lua.create_function(move |_, id: Option<u8>| {
            let state = lua_state(&state)?;
            Ok(id
                .map(|id| state.input & (1u32 << id.min(31)) != 0)
                .unwrap_or(state.input != 0))
        })?,
    )?;

    let state = shared.clone();
    globals.set(
        "btnp",
        lua.create_function(
            move |_, (id, hold, period): (Option<u8>, Option<i32>, Option<i32>)| {
                let state = lua_state(&state)?;
                let pressed = |button: usize| {
                    let bit = 1u32 << button;
                    let newly_pressed = state.input & bit != 0 && state.previous_input & bit == 0;
                    if newly_pressed {
                        return true;
                    }
                    let hold = hold.unwrap_or(-1);
                    let period = period.unwrap_or(-1);
                    hold >= 0
                        && period > 0
                        && state.holds[button] > hold as u32
                        && (state.holds[button] - hold as u32 - 1).is_multiple_of(period as u32)
                };
                Ok(id
                    .map(|id| pressed(id.min(31) as usize))
                    .unwrap_or_else(|| (0..32).any(pressed)))
            },
        )?,
    )?;

    let state = shared.clone();
    globals.set(
        "peek",
        lua.create_function(move |_, address: usize| {
            Ok(lua_state(&state)?.ram.get(address).copied().unwrap_or(0))
        })?,
    )?;
    let state = shared.clone();
    globals.set(
        "poke",
        lua.create_function(move |_, (address, value): (usize, u8)| {
            if let Some(byte) = lua_state(&state)?.ram.get_mut(address) {
                *byte = value;
            }
            Ok(())
        })?,
    )?;

    for (name, bits, write) in [
        ("peek1", 1u8, false),
        ("peek2", 2u8, false),
        ("peek4", 4u8, false),
        ("poke1", 1u8, true),
        ("poke2", 2u8, true),
        ("poke4", 4u8, true),
    ] {
        let state = shared.clone();
        if write {
            globals.set(
                name,
                lua.create_function(move |_, (index, value): (usize, u8)| {
                    bit_poke(&mut lua_state(&state)?.ram, index, bits, value);
                    Ok(())
                })?,
            )?;
        } else {
            globals.set(
                name,
                lua.create_function(move |_, index: usize| {
                    Ok(bit_peek(&lua_state(&state)?.ram, index, bits))
                })?,
            )?;
        }
    }

    let state = shared.clone();
    globals.set(
        "memset",
        lua.create_function(move |_, (address, value, size): (usize, u8, usize)| {
            let mut state = lua_state(&state)?;
            let end = address.saturating_add(size).min(state.ram.len());
            if address < end {
                state.ram[address..end].fill(value);
            }
            Ok(())
        })?,
    )?;
    let state = shared.clone();
    globals.set(
        "memcpy",
        lua.create_function(
            move |_, (destination, source, size): (usize, usize, usize)| {
                let mut state = lua_state(&state)?;
                if source <= state.ram.len()
                    && destination <= state.ram.len()
                    && size <= state.ram.len().saturating_sub(source)
                    && size <= state.ram.len().saturating_sub(destination)
                {
                    state.ram.copy_within(source..source + size, destination);
                }
                Ok(())
            },
        )?,
    )?;

    let state = shared.clone();
    globals.set(
        "mget",
        lua.create_function(move |_, (x, y): (usize, usize)| {
            let address = MAP_ADDR + y.saturating_mul(240) + x;
            Ok(lua_state(&state)?.ram.get(address).copied().unwrap_or(0))
        })?,
    )?;
    let state = shared.clone();
    globals.set(
        "mset",
        lua.create_function(move |_, (x, y, value): (usize, usize, u8)| {
            let address = MAP_ADDR + y.saturating_mul(240) + x;
            if let Some(byte) = lua_state(&state)?.ram.get_mut(address) {
                *byte = value;
            }
            Ok(())
        })?,
    )?;

    let state = shared.clone();
    globals.set(
        "fget",
        lua.create_function(move |_, (sprite, flag): (usize, Option<u8>)| {
            let value = lua_state(&state)?
                .ram
                .get(FLAGS_ADDR + sprite)
                .copied()
                .unwrap_or(0);
            Ok(flag
                .map(|flag| value & (1 << flag.min(7)) != 0)
                .unwrap_or(value != 0))
        })?,
    )?;
    let state = shared.clone();
    globals.set(
        "fset",
        lua.create_function(move |_, (sprite, flag, value): (usize, u8, Option<bool>)| {
            if let Some(flags) = lua_state(&state)?.ram.get_mut(FLAGS_ADDR + sprite) {
                let bit = 1 << flag.min(7);
                if value.unwrap_or(true) {
                    *flags |= bit;
                } else {
                    *flags &= !bit;
                }
            }
            Ok(())
        })?,
    )?;

    let state = shared.clone();
    globals.set(
        "spr",
        lua.create_function(
            move |_, (id, x, y, transparent, scale, flip, rotate, w, h): SpriteArgs| {
                let transparent = lua_transparent_colors(transparent);
                draw_sprite(
                    &mut *lua_state(&state)?,
                    id,
                    x,
                    y,
                    scale.unwrap_or(1).max(1),
                    flip.unwrap_or(0),
                    rotate.unwrap_or(0),
                    w.unwrap_or(1).max(1),
                    h.unwrap_or(1).max(1),
                    &transparent,
                );
                Ok(())
            },
        )?,
    )?;

    let state = shared.clone();
    globals.set(
        "map",
        lua.create_function(
            move |_,
                  (map_x, map_y, map_w, map_h, screen_x, screen_y, transparent, scale): MapArgs| {
                let transparent = lua_transparent_colors(transparent);
                let mut state = lua_state(&state)?;
                let map_x = map_x.unwrap_or(0);
                let map_y = map_y.unwrap_or(0);
                let map_w = map_w.unwrap_or(30).max(0);
                let map_h = map_h.unwrap_or(17).max(0);
                let scale = scale.unwrap_or(1).max(1);
                for y in 0..map_h {
                    for x in 0..map_w {
                        let address = MAP_ADDR
                            + (map_y + y).max(0) as usize * 240
                            + (map_x + x).max(0) as usize;
                        let tile = state.ram.get(address).copied().unwrap_or(0) as usize;
                        draw_sprite(
                            &mut state,
                            tile,
                            screen_x.unwrap_or(0) + x * 8 * scale,
                            screen_y.unwrap_or(0) + y * 8 * scale,
                            scale,
                            0,
                            0,
                            1,
                            1,
                            &transparent,
                        );
                    }
                }
                Ok(())
            },
        )?,
    )?;

    let state = shared.clone();
    globals.set(
        "pmem",
        lua.create_function(move |_, (index, value): (usize, Option<u32>)| {
            let address = 0x14004usize.saturating_add(index.saturating_mul(4));
            let mut state = lua_state(&state)?;
            if address + 4 > state.ram.len() || index >= 256 {
                return Ok(0);
            }
            let previous = u32::from_le_bytes(
                state.ram[address..address + 4]
                    .try_into()
                    .expect("four-byte slice"),
            );
            if let Some(value) = value {
                state.ram[address..address + 4].copy_from_slice(&value.to_le_bytes());
            }
            Ok(previous)
        })?,
    )?;

    let state = shared.clone();
    globals.set(
        "vbank",
        lua.create_function(move |_, bank: Option<u8>| {
            let mut state = lua_state(&state)?;
            let previous = state.vbank;
            if let Some(bank) = bank {
                state.vbank = bank.min(1);
            }
            Ok(previous)
        })?,
    )?;

    let state = shared.clone();
    globals.set(
        "clip",
        lua.create_function(
            move |_, (x, y, width, height): (Option<i32>, Option<i32>, Option<i32>, Option<i32>)| {
                lua_state(&state)?.clip = match (x, y, width, height) {
                    (Some(x), Some(y), Some(width), Some(height)) => [
                        x.clamp(0, WIDTH as i32),
                        y.clamp(0, HEIGHT as i32),
                        width.max(0).min(WIDTH as i32),
                        height.max(0).min(HEIGHT as i32),
                    ],
                    _ => [0, 0, WIDTH as i32, HEIGHT as i32],
                };
                Ok(())
            },
        )?,
    )?;

    let state = shared.clone();
    globals.set(
        "time",
        lua.create_function(move |_, ()| Ok(lua_state(&state)?.frame as f64 * 1000.0 / 60.0))?,
    )?;
    globals.set("tstamp", lua.create_function(|_, ()| Ok(0i64))?)?;

    let state = shared.clone();
    globals.set(
        "trace",
        lua.create_function(move |_, (message, _color): (String, Option<u8>)| {
            lua_state(&state)?.traces.push(message);
            Ok(())
        })?,
    )?;
    let state = shared.clone();
    globals.set(
        "exit",
        lua.create_function(move |_, ()| {
            lua_state(&state)?.exit_requested = true;
            Ok(())
        })?,
    )?;

    let state = shared.clone();
    globals.set(
        "print",
        lua.create_function(
            move |_, (text, x, y, color, fixed, scale, small): PrintArgs| {
                let _ = fixed;
                let scale = scale.unwrap_or(1).max(1);
                let width = if small.unwrap_or(false) { 4 } else { 6 };
                draw_debug_text(
                    &mut *lua_state(&state)?,
                    &text,
                    x.unwrap_or(0),
                    y.unwrap_or(0),
                    color.unwrap_or(15),
                    scale,
                    width,
                );
                Ok(text.chars().count() as i32 * width * scale)
            },
        )?,
    )?;

    globals.set(
        "mouse",
        lua.create_function(|_, ()| Ok((0u8, 0u8, false, false, false, 0i8, 0i8)))?,
    )?;
    globals.set("key", lua.create_function(|_, _: Option<u8>| Ok(false))?)?;
    globals.set(
        "keyp",
        lua.create_function(|_, _: (Option<u8>, Option<i32>, Option<i32>)| Ok(false))?,
    )?;

    // Sound is deliberately deterministic and currently observational only.
    // Calls are accepted so ordinary carts continue to run.
    for name in ["sfx", "music", "sync"] {
        globals.set(
            name,
            lua.create_function(|_, _: MultiValue| Ok(MultiValue::new()))?,
        )?;
    }
    Ok(())
}

fn draw_line(state: &mut RuntimeState, mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: u8) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        state.set_pixel(x0, y0, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let doubled = 2 * error;
        if doubled >= dy {
            error += dy;
            x0 += sx;
        }
        if doubled <= dx {
            error += dx;
            y0 += sy;
        }
    }
}

fn fill_triangle(state: &mut RuntimeState, points: [(i32, i32); 3], color: u8) {
    let min_x = points.iter().map(|point| point.0).min().unwrap_or(0);
    let max_x = points.iter().map(|point| point.0).max().unwrap_or(0);
    let min_y = points.iter().map(|point| point.1).min().unwrap_or(0);
    let max_y = points.iter().map(|point| point.1).max().unwrap_or(0);
    let edge = |a: (i32, i32), b: (i32, i32), p: (i32, i32)| {
        (p.0 - a.0) as i64 * (b.1 - a.1) as i64 - (p.1 - a.1) as i64 * (b.0 - a.0) as i64
    };
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let e0 = edge(points[0], points[1], (x, y));
            let e1 = edge(points[1], points[2], (x, y));
            let e2 = edge(points[2], points[0], (x, y));
            if (e0 >= 0 && e1 >= 0 && e2 >= 0) || (e0 <= 0 && e1 <= 0 && e2 <= 0) {
                state.set_pixel(x, y, color);
            }
        }
    }
}

fn bit_peek(bytes: &[u8], index: usize, bits: u8) -> u8 {
    let per_byte = 8 / bits as usize;
    let Some(byte) = bytes.get(index / per_byte) else {
        return 0;
    };
    let shift = (index % per_byte) * bits as usize;
    (*byte >> shift) & ((1u8 << bits) - 1)
}

fn bit_poke(bytes: &mut [u8], index: usize, bits: u8, value: u8) {
    let per_byte = 8 / bits as usize;
    let Some(byte) = bytes.get_mut(index / per_byte) else {
        return;
    };
    let shift = (index % per_byte) * bits as usize;
    let mask = ((1u8 << bits) - 1) << shift;
    *byte = (*byte & !mask) | ((value << shift) & mask);
}

fn lua_transparent_colors(value: Option<LuaValue>) -> BTreeSet<u8> {
    let mut result = BTreeSet::new();
    match value {
        Some(LuaValue::Integer(value)) => {
            result.insert(value as u8 & 0x0f);
        }
        Some(LuaValue::Number(value)) => {
            result.insert(value as u8 & 0x0f);
        }
        Some(LuaValue::Table(table)) => {
            for value in table.sequence_values::<u8>().flatten() {
                result.insert(value & 0x0f);
            }
        }
        _ => {}
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn draw_sprite(
    state: &mut RuntimeState,
    id: usize,
    x: i32,
    y: i32,
    scale: i32,
    flip: i32,
    rotate: i32,
    width: i32,
    height: i32,
    transparent: &BTreeSet<u8>,
) {
    for tile_y in 0..height {
        for tile_x in 0..width {
            let tile = id + tile_y as usize * 16 + tile_x as usize;
            let base = TILES_ADDR + tile.saturating_mul(32);
            for source_y in 0..8 {
                for source_x in 0..8 {
                    let pixel = source_y * 8 + source_x;
                    let Some(byte) = state.ram.get(base + pixel / 2).copied() else {
                        continue;
                    };
                    let color = if pixel & 1 == 0 {
                        byte & 0x0f
                    } else {
                        byte >> 4
                    };
                    if transparent.contains(&color) {
                        continue;
                    }
                    let mut px = if flip & 1 != 0 {
                        7 - source_x
                    } else {
                        source_x
                    } as i32;
                    let mut py = if flip & 2 != 0 {
                        7 - source_y
                    } else {
                        source_y
                    } as i32;
                    for _ in 0..rotate.rem_euclid(4) {
                        (px, py) = (7 - py, px);
                    }
                    for sy in 0..scale {
                        for sx in 0..scale {
                            state.set_pixel(
                                x + (tile_x * 8 + px) * scale + sx,
                                y + (tile_y * 8 + py) * scale + sy,
                                color,
                            );
                        }
                    }
                }
            }
        }
    }
}

// A small deterministic fallback glyph renderer. TIC-80's complete system font
// remains available in RAM for carts that draw it directly.
fn draw_debug_text(
    state: &mut RuntimeState,
    text: &str,
    x: i32,
    y: i32,
    color: u8,
    scale: i32,
    advance: i32,
) {
    for (index, byte) in text.bytes().enumerate() {
        for gy in 0..5 {
            for gx in 0..4 {
                if byte.rotate_left(gy as u32) & (1 << gx) != 0 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            state.set_pixel(
                                x + index as i32 * advance * scale + gx * scale + sx,
                                y + gy * scale + sy,
                                color,
                            );
                        }
                    }
                }
            }
        }
    }
}

fn framebuffer_rgba(state: &RuntimeState) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(WIDTH * HEIGHT * 4);
    let overlay_transparent = state.vram1[0x3ff8] & 0x0f;
    for pixel in 0..WIDTH * HEIGHT {
        let base_byte = state.ram[pixel / 2];
        let base_index = if pixel & 1 == 0 {
            base_byte & 0x0f
        } else {
            base_byte >> 4
        };
        let overlay_byte = state.vram1[pixel / 2];
        let overlay_index = if pixel & 1 == 0 {
            overlay_byte & 0x0f
        } else {
            overlay_byte >> 4
        };
        let (index, palette) = if overlay_index != overlay_transparent {
            (
                overlay_index as usize,
                &state.vram1[PALETTE_ADDR..PALETTE_ADDR + 48],
            )
        } else {
            let index = base_index as usize;
            (index, &state.ram[PALETTE_ADDR..PALETTE_ADDR + 48])
        };
        let fallback = SWEETIE_16[index];
        rgba.extend_from_slice(&[
            *palette.get(index * 3).unwrap_or(&fallback[0]),
            *palette.get(index * 3 + 1).unwrap_or(&fallback[1]),
            *palette.get(index * 3 + 2).unwrap_or(&fallback[2]),
            255,
        ]);
    }
    rgba
}
