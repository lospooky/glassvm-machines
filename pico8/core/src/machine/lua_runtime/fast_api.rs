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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cartridge, Pico8Runtime};

    unsafe extern "C-unwind" fn compare_decoders(state: *mut ffi::lua_State) -> c_int {
        unsafe {
            let top = ffi::lua_gettop(state);
            let mut expected = [0; 3];
            let mut count = 2;
            for index in 1..=3 {
                if index == 3
                    && matches!(ffi::lua_type(state, index), ffi::LUA_TNONE | ffi::LUA_TNIL)
                {
                    break;
                }
                let mut valid = 0;
                expected[index as usize - 1] = ffi::lua_tointegerx(state, index, &mut valid);
                if valid == 0 {
                    count = 0;
                    break;
                }
                if index == 3 {
                    count = 3;
                }
            }
            let expected_context = ffi::lua_touserdata(state, ffi::lua_upvalueindex(1));
            if ffi::lua_type(state, ffi::lua_upvalueindex(1)) != ffi::LUA_TLIGHTUSERDATA {
                count = 0;
            }
            let mut actual = [0; 3];
            let mut context = std::ptr::null_mut();
            let actual_count =
                glassvm_lua_decode_integer_pair(state, 1, actual.as_mut_ptr(), &mut context);
            let equal = actual_count == count
                && (count == 0
                    || (context == expected_context
                        && actual[..count as usize] == expected[..count as usize]))
                && ffi::lua_gettop(state) == top;
            ffi::lua_pushboolean(state, equal as c_int);
            1
        }
    }

    #[test]
    fn combined_decoder_matches_lua_api_without_changing_stack_depth() {
        let runtime = runtime("", false, 500_000);
        // Identity only: no callback dereferences this pointer.
        let pointer = LightUserData(std::ptr::dangling_mut::<u8>().cast());
        let check: Function = unsafe {
            runtime.lua.exec_raw(pointer, |state| {
                ffi::lua_pushcclosure(state, compare_decoders, 1);
            })
        }
        .unwrap();
        runtime.lua.globals().set("check_decoder", check).unwrap();
        for arguments in ["", "1", "1,2", "1,2,nil", "1,2,3,4,5", "nil,2,3"] {
            assert!(
                runtime
                    .lua
                    .load(format!("return check_decoder({arguments})"))
                    .eval::<bool>()
                    .unwrap()
            );
        }
        let values = [
            "0",
            "-0.0",
            "1.0",
            "1.5",
            "-1.5",
            "2147483648",
            "-2147483649",
            "9223372036854775807",
            "-9223372036854775807-1",
            "9223372036854775808.0",
            "0/0",
            "1/0",
            "-1/0",
            "'12'",
            "'12.5'",
            "'0x1f'",
            "'bad'",
            "'12\\0'",
            "string.char(255)",
            "false",
            "{}",
            "nil",
        ];
        for x in values {
            for y in values {
                for color in ["nil", "255", "'17'", "2.5", "{}"] {
                    let source = format!("return check_decoder({x},{y},{color})");
                    assert!(
                        runtime.lua.load(source).eval::<bool>().unwrap(),
                        "{x},{y},{color}"
                    );
                }
            }
        }
        // A missing context is rejected rather than yielding a borrowed pointer.
        let no_context = unsafe { runtime.lua.create_c_function(compare_decoders) }.unwrap();
        assert!(no_context.call::<bool>((1, 2, 3)).unwrap());
        let wrong_owner = runtime.lua.create_any_userdata(123_u64).unwrap();
        let wrong_context: Function = unsafe {
            runtime.lua.exec_raw(wrong_owner, |state| {
                ffi::lua_pushcclosure(state, compare_decoders, 1);
            })
        }
        .unwrap();
        assert!(wrong_context.call::<bool>((1, 2, 3)).unwrap());
    }

    fn runtime(source: &str, typed: bool, budget: u64) -> Pico8Runtime {
        let bytes =
            format!("pico-8 cartridge // http://www.pico-8.com\nversion 42\n__lua__\n{source}\n");
        let cart = Cartridge::parse(bytes.as_bytes()).unwrap();
        runtime_from_cartridge(&cart, typed, budget)
    }

    fn runtime_from_cartridge(cart: &Cartridge, typed: bool, budget: u64) -> Pico8Runtime {
        if typed {
            Pico8Runtime::new_with_api::<false>(cart, 7, budget, true).unwrap()
        } else {
            Pico8Runtime::new_with_lua_function_capture(cart, 7, budget, true).unwrap()
        }
    }

    #[test]
    fn real_cartridge_matches_typed_apis_frame_by_frame_with_all_activity() {
        let cartridge =
            Cartridge::parse(include_bytes!("../../../../fixtures/paper/fireintro.p8")).unwrap();
        let fast = runtime_from_cartridge(&cartridge, false, 500_000);
        let typed = runtime_from_cartridge(&cartridge, true, 500_000);
        for _ in 0..120 {
            assert_eq!(
                fast.step_frame_with_capture(true, true, true).unwrap(),
                typed.step_frame_with_capture(true, true, true).unwrap()
            );
            assert_eq!(fast.snapshot(), typed.snapshot());
            assert_eq!(fast.framebuffer(), typed.framebuffer());
        }
    }

    #[test]
    fn numeric_coercions_clipping_palette_and_retained_api_handles_match() {
        let source = "local pixel,random,floor=pset,rnd,flr
            function _update60() srand(19) camera(-3,2) clip(1,1,60,60) pal(2,9) color(2) end
            function _draw()
              pixel(1,4) pixel(2,4,nil) pixel(3,4,258,99)
              pixel('4','4','2') pixel(5.75,4.25,2.5)
              pixel(-2147483648,2147483647,-9223372036854775807)
              pixel(50,50,floor(random('16'))) pixel(40,40,floor(random(nil)))
              print(floor('-1.5'),floor(-0.0),floor(1/0),floor(-1/0))
              print(random(),random(16),random(-3),random(0))
            end";
        let fast = runtime(source, false, 500_000);
        let typed = runtime(source, true, 500_000);
        for _ in 0..4 {
            assert_eq!(
                fast.step_frame_with_capture(true, true, true).unwrap(),
                typed.step_frame_with_capture(true, true, true).unwrap()
            );
            assert_eq!(fast.snapshot(), typed.snapshot());
        }
    }

    #[test]
    fn floor_keeps_float_type_and_signed_zero() {
        let fast = runtime("", false, 500_000);
        let typed = runtime("", true, 500_000);
        for source in [
            "return flr(2)",
            "return flr(-0.0)",
            "return flr(-1.5)",
            "return flr(1/0)",
            "return flr(-1/0)",
            "return flr(0/0)",
            "return flr('2.5')",
        ] {
            let first = fast.lua.load(source).eval::<mlua::Value>().unwrap();
            let second = typed.lua.load(source).eval::<mlua::Value>().unwrap();
            assert!(matches!(first, mlua::Value::Number(_)), "{source}");
            let (mlua::Value::Number(first), mlua::Value::Number(second)) = (first, second) else {
                panic!("float type changed")
            };
            assert_eq!(first.to_bits(), second.to_bits(), "{source}");
        }
    }

    #[test]
    fn invalid_arguments_keep_typed_admission_and_do_not_mutate_state() {
        for source in [
            "flr()",
            "flr(nil)",
            "flr({})",
            "rnd({})",
            "rnd(false)",
            "pset()",
            "pset(1)",
            "pset(2147483648,0)",
            "pset(0,0,{})",
            "pset(0,0,1/0)",
        ] {
            let fast = runtime("", false, 500_000);
            let typed = runtime("", true, 500_000);
            let before = fast.snapshot();
            assert!(fast.lua.load(source).exec().is_err(), "{source}");
            assert!(typed.lua.load(source).exec().is_err(), "{source}");
            assert_eq!(fast.snapshot(), before, "{source}");
            assert_eq!(fast.snapshot(), typed.snapshot(), "{source}");
        }
    }

    #[test]
    fn budget_exhaustion_and_dynamic_api_replacement_match() {
        for source in [
            "function _draw() while true do pset(1,1,flr(rnd(16))) end end",
            "function _update60() pset=function() end rnd=function() return 4 end flr=function(x) return x end end\nfunction _draw() pset(1,1,flr(rnd())) end",
        ] {
            let fast = runtime(source, false, 2_000);
            let typed = runtime(source, true, 2_000);
            let first = fast.step_frame_with_capture(true, true, true);
            let second = typed.step_frame_with_capture(true, true, true);
            assert_eq!(first, second);
            assert_eq!(fast.snapshot(), typed.snapshot());
        }
    }

    #[test]
    fn lua_owns_fast_api_context_across_collection_and_thread_moves() {
        let source = "local pixel,random,floor=pset,rnd,flr
            function _draw() pixel(1,2,floor(random(16))) end";
        let fast = runtime(source, false, 500_000);
        let typed = runtime(source, true, 500_000);
        let moved = std::thread::spawn(move || {
            for _ in 0..8 {
                fast.lua.gc_collect().unwrap();
                fast.step_frame().unwrap();
            }
            fast.snapshot()
        })
        .join()
        .unwrap();
        for _ in 0..8 {
            typed.step_frame().unwrap();
        }
        assert_eq!(moved, typed.snapshot());
    }
}
