//! Finite hook projections for differential comparison with upstream Lua.
//! Run the same executable source with and without GLASSVM_LUA_REFERENCE_HOOK.

use std::sync::{Arc, Mutex};

use mlua::{DebugEvent, Function, HookTriggers, Lua, ThreadStatus, VmState};
use serde_json::{Value, json};

const SOURCE: &str = r#"
progress = 0
local function recurse(n)
  if n == 0 then return 1 end
  return recurse(n - 1)
end
function frame()
  for i = 1, 40 do
    progress = progress + recurse(3)
    host(progress)
    local objects = {}
    for j = 1, 8 do objects[j] = {i, j} end
  end
  return progress
end
"#;

fn basic(mask: u8, quantum: u32) -> Value {
    let lua = Lua::new();
    lua.globals()
        .set(
            "host",
            lua.create_function(|lua, value: i64| {
                if value % 7 == 0 {
                    lua.gc_collect()?;
                }
                Ok(value)
            })
            .unwrap(),
        )
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let output = Arc::clone(&events);
    lua.set_hook(
        HookTriggers {
            on_calls: mask & 1 != 0,
            on_returns: mask & 2 != 0,
            every_line: mask & 4 != 0,
            every_nth_instruction: (mask & 8 != 0).then_some(quantum),
        },
        move |_, debug| {
            output.lock().unwrap().push(json!({
                "event": format!("{:?}", debug.event()),
                "line": debug.curr_line(), "what": debug.source().what,
            }));
            Ok(VmState::Continue)
        },
    );
    lua.load(SOURCE).set_name("hook-probe").exec().unwrap();
    let frame: Function = lua.globals().get("frame").unwrap();
    let mut results = Vec::new();
    // Do not reinstall hooks at frame boundaries: count phase crosses calls.
    for _ in 0..4 {
        results.push(frame.call::<i64>(()).unwrap());
    }
    lua.remove_hook();
    let events = events.lock().unwrap();
    json!({"mask": mask, "quantum": quantum, "results": results, "events": *events})
}

fn exhaustion() -> Value {
    let lua = Lua::new();
    lua.load("progress=0; function run() while true do progress=progress+1 end end")
        .set_name("quota-probe")
        .exec()
        .unwrap();
    let ticks = Arc::new(Mutex::new(0_u32));
    let output = Arc::clone(&ticks);
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(17),
        move |_, _| {
            let mut count = output.lock().unwrap();
            *count += 1;
            if *count == 9 {
                return Err(mlua::Error::RuntimeError("quota exhausted".into()));
            }
            Ok(VmState::Continue)
        },
    );
    let error = lua
        .globals()
        .get::<Function>("run")
        .unwrap()
        .call::<()>(())
        .unwrap_err()
        .to_string();
    lua.remove_hook();
    json!({"ticks": *ticks.lock().unwrap(), "progress": lua.globals().get::<i64>("progress").unwrap(), "error": error})
}

fn yielding() -> Value {
    let lua = Lua::new();
    let function = lua
        .load("progress=0; for i=1,100 do progress=progress+i end; return progress")
        .set_name("yield-probe")
        .into_function()
        .unwrap();
    let thread = lua.create_thread(function).unwrap();
    let ticks = Arc::new(Mutex::new(Vec::new()));
    let output = Arc::clone(&ticks);
    thread.set_hook(
        HookTriggers::new().every_nth_instruction(13),
        move |lua, debug| {
            assert_eq!(debug.event(), DebugEvent::Count);
            output
                .lock()
                .unwrap()
                .push(lua.globals().get::<i64>("progress").unwrap_or(0));
            Ok(VmState::Yield)
        },
    );
    let mut resumes = Vec::new();
    while thread.status() == ThreadStatus::Resumable {
        let value: Option<i64> = thread.resume(()).unwrap();
        resumes.push(value);
        assert!(resumes.len() < 1000);
    }
    json!({"ticks": *ticks.lock().unwrap(), "resumes": resumes})
}

pub fn projection() -> Value {
    let mut cases = Vec::new();
    // All count/call/return combinations, plus line hooks with and without
    // count hooks. Quanta 1 and 2 exercise the expiration/fast-path boundary.
    for mask in [0, 1, 2, 3, 4, 7, 8, 9, 10, 11, 12, 15] {
        for quantum in [1, 2, 17, 1000] {
            cases.push(basic(mask, quantum));
        }
    }
    json!({"cases": cases, "exhaustion": exhaustion(), "yielding": yielding()})
}

fn main() {
    println!("{}", projection());
}
