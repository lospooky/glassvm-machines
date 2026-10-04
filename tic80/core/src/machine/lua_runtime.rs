use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use mlua::{
    FromLua, Function, HookTriggers, Lua, LuaOptions, MultiValue, StdLib, Value as LuaValue,
    VmState,
};

use crate::configuration::{
    FLAGS_ADDR, GAMEPAD_ADDR, HEIGHT, LUA_HOOK_GRANULARITY, LUA_INSTRUCTION_BUDGET,
    LUA_MEMORY_LIMIT_BYTES, MAP_ADDR, PALETTE_ADDR, PALETTE_MAP_ADDR, SCREEN_BYTES, TILES_ADDR,
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
    Option<Function>,
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
    scanline_palettes: Vec<[u8; 96]>,
    frame: u64,
    input: u32,
    previous_input: u32,
    holds: [u32; 32],
    clip: [i32; 4],
    pending_traces: Vec<String>,
    trace_count: u64,
    capture_traces: bool,
    exit_requested: bool,
}

impl RuntimeState {
    fn from_cart(cart: &ParsedCart, capture_traces: bool) -> Self {
        let mut vram1 = vec![0; VRAM_BYTES];
        install_identity_palette_map(&mut vram1);
        let ram = cart.initial_ram.clone();
        let palette_pair = palette_pair(&ram, &vram1);
        Self {
            ram,
            vram1,
            vbank: 0,
            scanline_palettes: vec![palette_pair; HEIGHT],
            frame: 0,
            input: 0,
            previous_input: 0,
            holds: [0; 32],
            clip: [0, 0, WIDTH as i32, HEIGHT as i32],
            pending_traces: Vec::new(),
            trace_count: 0,
            capture_traces,
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

    fn read_memory(&self, address: usize) -> u8 {
        if address < VRAM_BYTES {
            self.active_vram()[address]
        } else {
            self.ram.get(address).copied().unwrap_or(0)
        }
    }

    fn write_memory(&mut self, address: usize, value: u8) {
        if address < VRAM_BYTES {
            self.active_vram_mut()[address] = value;
        } else if let Some(byte) = self.ram.get_mut(address) {
            *byte = value;
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
        let color = self.map_color(color);
        let byte = &mut self.active_vram_mut()[pixel / 2];
        if pixel & 1 == 0 {
            *byte = (*byte & 0xf0) | (color & 0x0f);
        } else {
            *byte = (*byte & 0x0f) | ((color & 0x0f) << 4);
        }
    }

    fn map_color(&self, color: u8) -> u8 {
        let color = color & 0x0f;
        let mapping = self.active_vram()[PALETTE_MAP_ADDR + color as usize / 2];
        if color & 1 == 0 {
            mapping & 0x0f
        } else {
            mapping >> 4
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

fn install_identity_palette_map(vram: &mut [u8]) {
    for index in 0..8 {
        vram[PALETTE_MAP_ADDR + index] = (index as u8 * 2) | ((index as u8 * 2 + 1) << 4);
    }
}

pub struct Tic80Runtime {
    cart_bytes: Vec<u8>,
    seed: u64,
    lua: Lua,
    shared: Arc<Mutex<RuntimeState>>,
    instructions_remaining: Arc<AtomicU64>,
}

fn lua_state(state: &Arc<Mutex<RuntimeState>>) -> mlua::Result<MutexGuard<'_, RuntimeState>> {
    state
        .lock()
        .map_err(|_| mlua::Error::RuntimeError("TIC-80 state lock poisoned".into()))
}

impl Tic80Runtime {
    pub fn new(cart_bytes: &[u8], seed: u64) -> Result<Self, String> {
        Self::new_with_trace_capture(cart_bytes, seed, true)
    }

    pub fn new_with_trace_capture(
        cart_bytes: &[u8],
        seed: u64,
        capture_traces: bool,
    ) -> Result<Self, String> {
        let cart = parse_cart(cart_bytes)?;
        if cart.language != "lua" {
            return Err(format!(
                "unsupported TIC-80 cartridge language {:?}; this bundle executes Lua carts through its bounded Lua 5.4 compatibility runtime",
                cart.language
            ));
        }
        let shared = Arc::new(Mutex::new(RuntimeState::from_cart(&cart, capture_traces)));
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
                let mut remaining = hook_budget.load(Ordering::Relaxed);
                loop {
                    let Some(updated) = remaining.checked_sub(LUA_HOOK_GRANULARITY) else {
                        return Err(mlua::Error::RuntimeError(
                            "TIC-80 instruction budget exhausted".into(),
                        ));
                    };
                    match hook_budget.compare_exchange_weak(
                        remaining,
                        updated,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break,
                        Err(actual) => remaining = actual,
                    }
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
            instructions_remaining,
        })
    }

    pub fn tick(&mut self, input: u32) -> Result<FrameOutcome, String> {
        self.tick_with_trace_capture(input, true)
    }

    pub fn tick_with_trace_capture(
        &mut self,
        input: u32,
        capture_traces: bool,
    ) -> Result<FrameOutcome, String> {
        {
            let mut state = self
                .shared
                .lock()
                .map_err(|_| "TIC-80 state lock poisoned")?;
            state.set_input(input);
            if !capture_traces {
                state.pending_traces.clear();
            }
            state.capture_traces = capture_traces;
        }
        let tic = self
            .lua
            .globals()
            .get::<Function>("TIC")
            .map_err(|error| format!("TIC-80 TIC callback missing: {error}"))?;
        self.instructions_remaining
            .store(LUA_INSTRUCTION_BUDGET, Ordering::Relaxed);
        if let Err(error) = tic.call::<()>(()) {
            if let Ok(mut state) = self.shared.lock() {
                state.pending_traces.clear();
                state.capture_traces = false;
            }
            return Err(format!("TIC-80 TIC callback failed: {error}"));
        }
        {
            let mut state = self
                .shared
                .lock()
                .map_err(|_| "TIC-80 state lock poisoned")?;
            let palette_pair = palette_pair(&state.ram, &state.vram1);
            state.scanline_palettes.fill(palette_pair);
        }
        if let Ok(bdr) = self.lua.globals().get::<Function>("BDR") {
            for scanline in 0..144 {
                if let Err(error) = bdr.call::<()>((scanline,)) {
                    if let Ok(mut state) = self.shared.lock() {
                        state.pending_traces.clear();
                        state.capture_traces = false;
                    }
                    return Err(format!(
                        "TIC-80 BDR callback failed at scanline {scanline}: {error}"
                    ));
                }
                if (4..140).contains(&scanline) {
                    let mut state = self
                        .shared
                        .lock()
                        .map_err(|_| "TIC-80 state lock poisoned")?;
                    let palette_pair = palette_pair(&state.ram, &state.vram1);
                    state.scanline_palettes[scanline - 4] = palette_pair;
                }
            }
        }
        let mut state = self
            .shared
            .lock()
            .map_err(|_| "TIC-80 state lock poisoned")?;
        state.frame += 1;
        let frame = state.frame;
        let exit_requested = state.exit_requested;
        let traces = std::mem::take(&mut state.pending_traces);
        drop(state);
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

    pub fn artifact_bytes(&self) -> &[u8] {
        &self.cart_bytes
    }

    pub fn frame(&self) -> Result<u64, String> {
        Ok(self
            .shared
            .lock()
            .map_err(|_| "TIC-80 state lock poisoned")?
            .frame)
    }

    pub fn trace_count(&self) -> Result<u64, String> {
        Ok(self
            .shared
            .lock()
            .map_err(|_| "TIC-80 state lock poisoned")?
            .trace_count)
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
            exit_requested: state.exit_requested,
        })
    }

    /// Reconstruct hidden Lua state from caller-supplied scheduled inputs.
    /// Inputs are consumed as an iterator and never retained by the runtime.
    pub fn restore_snapshot_from_inputs(
        &mut self,
        expected: &RuntimeSnapshot,
        inputs: impl IntoIterator<Item = u32>,
        capture_traces: bool,
    ) -> Result<(), String> {
        let mut candidate =
            Self::new_with_trace_capture(&self.cart_bytes, self.seed, capture_traces)?;
        for input in inputs {
            candidate.tick_with_trace_capture(input, false)?;
        }
        let actual = candidate.snapshot()?;
        if !same_machine_state(&actual, expected) {
            return Err(
                "TIC-80 session snapshot cannot reconstruct hidden machine state from the prepared input schedule"
                    .into(),
            );
        }
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
            let mut state = lua_state(&state)?;
            let color = color.unwrap_or(0);
            if state.clip == [0, 0, WIDTH as i32, HEIGHT as i32] {
                let mapped = state.map_color(color);
                state.active_vram_mut()[..SCREEN_BYTES].fill(mapped | (mapped << 4));
            } else {
                let [x, y, width, height] = state.clip;
                for row in y..y + height {
                    for column in x..x + width {
                        state.set_pixel(column, row, color);
                    }
                }
            }
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
        "ttri",
        lua.create_function(move |lua, args: MultiValue| {
            let mut args = args.into_iter();
            let mut coordinates = [0.0; 12];
            for coordinate in &mut coordinates {
                *coordinate = read_ttri_number(&mut args, lua, "coordinate")?;
                if !coordinate.is_finite() {
                    return Err(mlua::Error::RuntimeError(
                        "ttri coordinates must be finite".into(),
                    ));
                }
            }
            let texture_source = i32::from_lua(args.next().unwrap_or(LuaValue::Integer(0)), lua)?;
            let color_key = args.next().unwrap_or(LuaValue::Integer(-1));
            let z = [
                read_ttri_optional_number(&mut args, lua)?,
                read_ttri_optional_number(&mut args, lua)?,
                read_ttri_optional_number(&mut args, lua)?,
            ];
            if z.iter().any(|value| *value != 0.0) {
                return Err(mlua::Error::RuntimeError(
                    "ttri perspective/depth coordinates are not supported yet".into(),
                ));
            }
            let transparent = ttri_transparent_colors(color_key)?;
            let vertices = [
                TtriVertex {
                    x: coordinates[0],
                    y: coordinates[1],
                    u: coordinates[6],
                    v: coordinates[7],
                },
                TtriVertex {
                    x: coordinates[2],
                    y: coordinates[3],
                    u: coordinates[8],
                    v: coordinates[9],
                },
                TtriVertex {
                    x: coordinates[4],
                    y: coordinates[5],
                    u: coordinates[10],
                    v: coordinates[11],
                },
            ];
            let mut state = lua_state(&state)?;
            draw_textured_triangle(&mut state, vertices, texture_source, &transparent)
                .map_err(mlua::Error::RuntimeError)
        })?,
    )?;

    let state = shared.clone();
    globals.set(
        "btn",
        lua.create_function(move |_, id: Option<u8>| {
            let state = lua_state(&state)?;
            Ok(match id {
                Some(id) => LuaValue::Boolean(state.input & (1u32 << id.min(31)) != 0),
                None => LuaValue::Integer(state.input as i64),
            })
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
                Ok(match id {
                    Some(id) => LuaValue::Boolean(pressed(id.min(31) as usize)),
                    None => {
                        let mask = (0..32).fold(0u32, |mask, button| {
                            if pressed(button) {
                                mask | (1u32 << button)
                            } else {
                                mask
                            }
                        });
                        LuaValue::Integer(mask as i64)
                    }
                })
            },
        )?,
    )?;

    let state = shared.clone();
    globals.set(
        "peek",
        lua.create_function(move |_, address: usize| Ok(lua_state(&state)?.read_memory(address)))?,
    )?;
    let state = shared.clone();
    globals.set(
        "poke",
        lua.create_function(move |_, (address, value): (usize, u8)| {
            lua_state(&state)?.write_memory(address, value);
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
                    let mut state = lua_state(&state)?;
                    bit_poke(&mut state, index, bits, value);
                    Ok(())
                })?,
            )?;
        } else {
            globals.set(
                name,
                lua.create_function(move |_, index: usize| {
                    let state = lua_state(&state)?;
                    Ok(bit_peek(&state, index, bits))
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
            for address in address..end {
                state.write_memory(address, value);
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
                    let bytes = (source..source + size)
                        .map(|address| state.read_memory(address))
                        .collect::<Vec<_>>();
                    for (offset, byte) in bytes.into_iter().enumerate() {
                        state.write_memory(destination + offset, byte);
                    }
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
                    scale.unwrap_or(1),
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
                  (map_x, map_y, map_w, map_h, screen_x, screen_y, transparent, scale, remap): MapArgs| {
                let transparent = lua_transparent_colors(transparent);
                let map_x = map_x.unwrap_or(0);
                let map_y = map_y.unwrap_or(0);
                let map_w = map_w.unwrap_or(30).max(0);
                let map_h = map_h.unwrap_or(17).max(0);
                let scale = scale.unwrap_or(1).max(1);
                for y in 0..map_h {
                    for x in 0..map_w {
                        let cell_x = (i64::from(map_x) + i64::from(x)).rem_euclid(240) as i32;
                        let cell_y = (i64::from(map_y) + i64::from(y)).rem_euclid(136) as i32;
                        let address = MAP_ADDR + cell_y as usize * 240 + cell_x as usize;
                        let tile = {
                            let state = lua_state(&state)?;
                            state.ram.get(address).copied().unwrap_or(0) as i32
                        };
                        let (tile, flip, rotate) = if let Some(remap) = &remap {
                            let (tile, flip, rotate) = remap.call::<(
                                i32,
                                Option<i32>,
                                Option<i32>,
                            )>((tile, cell_x, cell_y))?;
                            let flip = flip.unwrap_or(0);
                            let rotate = rotate.unwrap_or(0);
                            if !(0..=511).contains(&tile) {
                                return Err(mlua::Error::RuntimeError(format!(
                                    "TIC-80 map remap returned tile {tile}; expected 0..511"
                                )));
                            }
                            if !(0..=3).contains(&flip) {
                                return Err(mlua::Error::RuntimeError(format!(
                                    "TIC-80 map remap returned flip {flip}; expected 0..3"
                                )));
                            }
                            if !(0..=3).contains(&rotate) {
                                return Err(mlua::Error::RuntimeError(format!(
                                    "TIC-80 map remap returned rotation {rotate}; expected 0..3"
                                )));
                            }
                            (tile as usize, flip, rotate)
                        } else {
                            (tile as usize, 0, 0)
                        };
                        let mut state = lua_state(&state)?;
                        draw_sprite(
                            &mut state,
                            tile,
                            screen_x.unwrap_or(0) + x * 8 * scale,
                            screen_y.unwrap_or(0) + y * 8 * scale,
                            scale,
                            flip,
                            rotate,
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
                    (Some(x), Some(y), Some(width), Some(height)) => {
                        let left = x.clamp(0, WIDTH as i32);
                        let top = y.clamp(0, HEIGHT as i32);
                        let right = (i64::from(x) + i64::from(width.max(0)))
                            .clamp(i64::from(left), WIDTH as i64) as i32;
                        let bottom = (i64::from(y) + i64::from(height.max(0)))
                            .clamp(i64::from(top), HEIGHT as i64) as i32;
                        [left, top, right - left, bottom - top]
                    }
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
            let mut state = lua_state(&state)?;
            state.trace_count = state.trace_count.saturating_add(1);
            if state.capture_traces {
                state.pending_traces.push(message);
            }
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

#[derive(Clone, Copy)]
struct TtriVertex {
    x: f64,
    y: f64,
    u: f64,
    v: f64,
}

fn read_ttri_number(
    args: &mut impl Iterator<Item = LuaValue>,
    lua: &Lua,
    name: &str,
) -> mlua::Result<f64> {
    f64::from_lua(args.next().unwrap_or(LuaValue::Nil), lua)
        .map_err(|_| mlua::Error::RuntimeError(format!("ttri requires numeric {name} values")))
}

fn read_ttri_optional_number(
    args: &mut impl Iterator<Item = LuaValue>,
    lua: &Lua,
) -> mlua::Result<f64> {
    match args.next().unwrap_or(LuaValue::Nil) {
        LuaValue::Nil => Ok(0.0),
        value => {
            let number = f64::from_lua(value, lua).map_err(|_| {
                mlua::Error::RuntimeError("ttri depth values must be numbers".into())
            })?;
            if !number.is_finite() {
                return Err(mlua::Error::RuntimeError(
                    "ttri depth values must be finite".into(),
                ));
            }
            Ok(number)
        }
    }
}

fn ttri_transparent_colors(value: LuaValue) -> mlua::Result<BTreeSet<u8>> {
    match value {
        LuaValue::Integer(-1) => Ok(BTreeSet::new()),
        LuaValue::Number(-1.0) => Ok(BTreeSet::new()),
        LuaValue::Integer(value) if (0..=15).contains(&value) => Ok(BTreeSet::from([value as u8])),
        LuaValue::Number(value) if value.fract() == 0.0 && (0.0..=15.0).contains(&value) => {
            Ok(BTreeSet::from([value as u8]))
        }
        LuaValue::Table(_) => Ok(lua_transparent_colors(Some(value))),
        _ => Err(mlua::Error::RuntimeError(
            "ttri chromakey must be -1, a palette index, or an index array".into(),
        )),
    }
}

fn draw_textured_triangle(
    state: &mut RuntimeState,
    mut vertices: [TtriVertex; 3],
    texture_source: i32,
    transparent: &BTreeSet<u8>,
) -> Result<(), String> {
    if !(0..=2).contains(&texture_source) {
        return Err(format!(
            "unsupported ttri texture source {texture_source}; expected 0, 1, or 2"
        ));
    }
    if vertices.iter().any(|vertex| {
        !vertex.x.is_finite()
            || !vertex.y.is_finite()
            || !vertex.u.is_finite()
            || !vertex.v.is_finite()
            || vertex.x.abs() > 1.0e9
            || vertex.y.abs() > 1.0e9
            || vertex.u.abs() > 1.0e9
            || vertex.v.abs() > 1.0e9
    }) {
        return Err("ttri coordinates must be finite and within +/-1e9".into());
    }

    let edge = |a: TtriVertex, b: TtriVertex, x: f64, y: f64| {
        (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x)
    };
    let mut area = edge(vertices[0], vertices[1], vertices[2].x, vertices[2].y);
    if area.floor() == 0.0 {
        return Ok(());
    }
    if area < 0.0 {
        vertices.swap(1, 2);
        area = -area;
    }

    let min_x = vertices
        .iter()
        .map(|vertex| vertex.x)
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(state.clip[0] as f64) as i32;
    let min_y = vertices
        .iter()
        .map(|vertex| vertex.y)
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(state.clip[1] as f64) as i32;
    let max_x = vertices
        .iter()
        .map(|vertex| vertex.x)
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        .min((state.clip[0] + state.clip[2]) as f64) as i32;
    let max_y = vertices
        .iter()
        .map(|vertex| vertex.y)
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        .min((state.clip[1] + state.clip[3]) as f64) as i32;
    if min_x >= max_x || min_y >= max_y {
        return Ok(());
    }

    let center_offset = 0.5 - f32::EPSILON as f64;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let point_x = x as f64 + center_offset;
            let point_y = y as f64 + center_offset;
            let weights = [
                edge(vertices[1], vertices[2], point_x, point_y) / area,
                edge(vertices[2], vertices[0], point_x, point_y) / area,
                edge(vertices[0], vertices[1], point_x, point_y) / area,
            ];
            if weights.iter().any(|weight| *weight < -f64::EPSILON) {
                continue;
            }
            let u = weights
                .iter()
                .zip(vertices)
                .map(|(weight, vertex)| weight * vertex.u)
                .sum::<f64>();
            let v = weights
                .iter()
                .zip(vertices)
                .map(|(weight, vertex)| weight * vertex.v)
                .sum::<f64>();
            let color =
                sample_ttri_texture(state, texture_source, u.floor() as i64, v.floor() as i64);
            if !transparent.contains(&color) {
                state.set_pixel(x, y, color);
            }
        }
    }
    Ok(())
}

fn sample_ttri_texture(state: &RuntimeState, source: i32, u: i64, v: i64) -> u8 {
    match source {
        0 => {
            let x = u.rem_euclid(128) as usize;
            let y = v.rem_euclid(256) as usize;
            let tile = (y / 8) * 16 + x / 8;
            let pixel = (y % 8) * 8 + x % 8;
            let byte = state.read_memory(TILES_ADDR + tile * 32 + pixel / 2);
            if pixel & 1 == 0 {
                byte & 0x0f
            } else {
                byte >> 4
            }
        }
        1 => {
            let x = u.rem_euclid(240 * 8) as usize;
            let y = v.rem_euclid(136 * 8) as usize;
            let map_address = MAP_ADDR + (y / 8) * 240 + x / 8;
            let tile = state.ram.get(map_address).copied().unwrap_or(0) as usize;
            let pixel = (y % 8) * 8 + x % 8;
            let address = TILES_ADDR + tile * 32 + pixel / 2;
            let byte = state.read_memory(address);
            if pixel & 1 == 0 {
                byte & 0x0f
            } else {
                byte >> 4
            }
        }
        2 => {
            let x = u.rem_euclid(WIDTH as i64) as usize;
            let y = v.rem_euclid(HEIGHT as i64) as usize;
            let pixel = y * WIDTH + x;
            let vram = if state.vbank == 0 {
                &state.vram1
            } else {
                &state.ram[..VRAM_BYTES]
            };
            let byte = vram[pixel / 2];
            if pixel & 1 == 0 {
                byte & 0x0f
            } else {
                byte >> 4
            }
        }
        _ => unreachable!("texture source validated before sampling"),
    }
}

fn bit_peek(state: &RuntimeState, index: usize, bits: u8) -> u8 {
    let per_byte = 8 / bits as usize;
    let byte = state.read_memory(index / per_byte);
    let shift = (index % per_byte) * bits as usize;
    (byte >> shift) & ((1u8 << bits) - 1)
}

fn bit_poke(state: &mut RuntimeState, index: usize, bits: u8, value: u8) {
    let per_byte = 8 / bits as usize;
    let address = index / per_byte;
    let shift = (index % per_byte) * bits as usize;
    let mask = ((1u8 << bits) - 1) << shift;
    let byte = state.read_memory(address);
    state.write_memory(address, (byte & !mask) | ((value << shift) & mask));
}

fn lua_transparent_colors(value: Option<LuaValue>) -> BTreeSet<u8> {
    let mut result = BTreeSet::new();
    match value {
        Some(LuaValue::Integer(value)) if (0..=15).contains(&value) => {
            result.insert(value as u8 & 0x0f);
        }
        Some(LuaValue::Number(value)) if (0.0..=15.0).contains(&value) => {
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
    if scale <= 0 || width <= 0 || height <= 0 {
        return;
    }
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
                    let mut px = source_x as i32;
                    let mut py = source_y as i32;
                    for _ in 0..rotate.rem_euclid(4) {
                        (px, py) = (7 - py, px);
                    }
                    if flip & 1 != 0 {
                        px = 7 - px;
                    }
                    if flip & 2 != 0 {
                        py = 7 - py;
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
    for y in 0..HEIGHT {
        let palette_pair = &state.scanline_palettes[y];
        let base_palette = &palette_pair[..48];
        let overlay_palette = &palette_pair[48..];
        for x in 0..WIDTH {
            let pixel = y * WIDTH + x;
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
                (overlay_index as usize, overlay_palette)
            } else {
                (base_index as usize, base_palette)
            };
            let fallback = SWEETIE_16[index];
            rgba.extend_from_slice(&[
                *palette.get(index * 3).unwrap_or(&fallback[0]),
                *palette.get(index * 3 + 1).unwrap_or(&fallback[1]),
                *palette.get(index * 3 + 2).unwrap_or(&fallback[2]),
                255,
            ]);
        }
    }
    rgba
}

fn palette_pair(ram: &[u8], vram1: &[u8]) -> [u8; 96] {
    let mut pair = [0; 96];
    pair[..48].copy_from_slice(&ram[PALETTE_ADDR..PALETTE_ADDR + 48]);
    pair[48..].copy_from_slice(&vram1[PALETTE_ADDR..PALETTE_ADDR + 48]);
    pair
}
