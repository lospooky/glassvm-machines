//! Numeric fast paths for the hottest existing APIs.
//!
//! The typed callbacks remain responsible for unusual argument coercions and
//! diagnostics. Common numeric calls avoid mlua's generic callback dispatch.
//! No Lua instructions, globals, machine state, or observation rules change.

use std::os::raw::c_int;

use mlua::{Function, LightUserData, Lua, ffi};

use super::{Arc, Mutex, RngInstrumentation, RuntimeSnapshot, draw_pixel, lock};

unsafe extern "C" {
    // Private to the vendored Lua build, not a GlassVM/public Lua contract.
    // Reads two required integers, an optional third, and a context upvalue.
    // Returns 0 on failed conversion, 2 without the third value, 3 with it.
    fn glassvm_lua_decode_integer_pair(
        state: *mut ffi::lua_State,
        context_upvalue: c_int,
        values: *mut ffi::lua_Integer,
        context: *mut *mut std::ffi::c_void,
    ) -> c_int;
}

struct Context {
    state: Arc<Mutex<RuntimeSnapshot>>,
    rng: Arc<RngInstrumentation>,
}

pub(super) fn install(
    lua: &Lua,
    state: Arc<Mutex<RuntimeSnapshot>>,
    rng: Arc<RngInstrumentation>,
) -> Result<(), String> {
    // A Lua-owned Box keeps the address stable when the runtime moves. Every
    // closure retains that owner as upvalue 2 for at least as long as its raw
    // pointer (upvalue 3) can be used. The sandbox exposes no upvalue mutation.
    let context = Box::new(Context { state, rng });
    let pointer = LightUserData((&*context as *const Context).cast_mut().cast());
    let owner = lua
        .create_any_userdata(context)
        .map_err(|error| error.to_string())?;
    let globals = lua.globals();
    for (name, callback) in [
        ("flr", floor as ffi::lua_CFunction),
        ("rnd", random as ffi::lua_CFunction),
        ("pset", pixel as ffi::lua_CFunction),
    ] {
        let typed = globals
            .get::<Function>(name)
            .map_err(|error| error.to_string())?;
        // SAFETY: all upvalues are pushed before constructing the closure. The
        // callbacks below obey Lua's stack, ownership, and unwind requirements.
        let function: Function = unsafe {
            lua.exec_raw((typed, owner.clone(), pointer), |state| {
                ffi::lua_pushcclosure(state, callback, 3);
            })
        }
        .map_err(|error| error.to_string())?;
        globals
            .set(name, function)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

// SAFETY for every callback: Lua invokes it while this Lua state is exclusively
// executing. Its three upvalues were installed above and cannot be changed by
// cartridge code. No owned Rust value or lock guard is live across a Lua call
// that can raise an error/longjmp. Numeric pushes do not allocate or raise.
unsafe extern "C-unwind" fn floor(state: *mut ffi::lua_State) -> c_int {
    unsafe {
        let mut valid = 0;
        let number = ffi::lua_tonumberx(state, 1, &mut valid);
        if valid == 0 {
            return typed_call(state);
        }
        // Keep the existing f64 result, including signed zero and non-finites;
        // Lua's math.floor would instead change some results to integers.
        ffi::lua_pushnumber(state, number.floor());
        1
    }
}

unsafe extern "C-unwind" fn random(state: *mut ffi::lua_State) -> c_int {
    unsafe {
        let upper = match ffi::lua_type(state, 1) {
            ffi::LUA_TNONE | ffi::LUA_TNIL => 1.0,
            ffi::LUA_TNUMBER => ffi::lua_tonumber(state, 1),
            _ => return typed_call(state),
        };
        let context = &*ffi::lua_touserdata(state, ffi::lua_upvalueindex(3)).cast::<Context>();
        let number = {
            let mut snapshot = lock(&context.state);
            let number = super::next_random(&mut snapshot, upper);
            context.rng.record_random();
            number
        };
        ffi::lua_pushnumber(state, number);
        1
    }
}

unsafe extern "C-unwind" fn pixel(state: *mut ffi::lua_State) -> c_int {
    unsafe {
        let mut values = [0; 3];
        let mut context = std::ptr::null_mut();
        let count = glassvm_lua_decode_integer_pair(state, 3, values.as_mut_ptr(), &mut context);
        if count == 0 {
            return typed_call(state);
        }
        let Ok(x) = i32::try_from(values[0]) else {
            return typed_call(state);
        };
        let Ok(y) = i32::try_from(values[1]) else {
            return typed_call(state);
        };
        // SAFETY: the decoder returns only the light-userdata pointer rooted
        // by this closure's Lua-owned Context, never a Lua-stack reference.
        let context = &*context.cast::<Context>();
        {
            let mut snapshot = lock(&context.state);
            let color = if count == 3 {
                values[2] as u8
            } else {
                snapshot.draw_color
            };
            draw_pixel(&mut snapshot, x, y, color);
            snapshot.draw_calls += 1;
        }
        0
    }
}

unsafe fn typed_call(state: *mut ffi::lua_State) -> c_int {
    unsafe {
        let nargs = ffi::lua_gettop(state);
        // Forward the original argument list, including missing arguments and
        // extra values, to the canonical typed implementation. Only trivial
        // Rust locals exist here if Lua raises the original conversion error.
        ffi::lua_pushvalue(state, ffi::lua_upvalueindex(1));
        ffi::lua_insert(state, 1);
        ffi::lua_call(state, nargs, ffi::LUA_MULTRET);
        ffi::lua_gettop(state)
    }
}
