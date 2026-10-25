//! Native frame execution results.

use crate::event::Tic80Event;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Successful compatibility RNG API calls, not generated values or RNG state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RngActivity {
    pub random_calls: u64,
    pub reseed_calls: u64,
}

/// Observation state is deliberately separate from resumable machine state.
#[derive(Default)]
pub(crate) struct RngInstrumentation {
    enabled: AtomicBool,
    random_calls: AtomicU64,
    reseed_calls: AtomicU64,
}

impl RngInstrumentation {
    pub(crate) fn capture(&self, enabled: bool) -> RngCapture<'_> {
        if enabled {
            self.random_calls.store(0, Ordering::Relaxed);
            self.reseed_calls.store(0, Ordering::Relaxed);
        }
        self.enabled.store(enabled, Ordering::Relaxed);
        RngCapture {
            instrumentation: self,
            enabled,
        }
    }

    pub(crate) fn record_random(&self) {
        if self.enabled.load(Ordering::Relaxed) {
            self.random_calls.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_reseed(&self) {
        if self.enabled.load(Ordering::Relaxed) {
            self.reseed_calls.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(crate) struct RngCapture<'a> {
    instrumentation: &'a RngInstrumentation,
    enabled: bool,
}

impl RngCapture<'_> {
    pub(crate) fn output(&self) -> Option<RngActivity> {
        self.enabled.then(|| RngActivity {
            random_calls: self.instrumentation.random_calls.load(Ordering::Relaxed),
            reseed_calls: self.instrumentation.reseed_calls.load(Ordering::Relaxed),
        })
    }
}

impl Drop for RngCapture<'_> {
    fn drop(&mut self) {
        self.instrumentation.enabled.store(false, Ordering::Relaxed);
    }
}

/// Coarse runtime accounting for one successfully completed frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionActivity {
    pub main_calls: u64,
    pub border_calls: u64,
    pub instruction_hook_ticks: u64,
}

/// Calls and returns for Lua-defined functions, excluding host API bindings.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LuaFunctionActivity {
    pub calls: u64,
    pub returns: u64,
}

#[derive(Default)]
pub(crate) struct LuaFunctionInstrumentation {
    enabled: AtomicBool,
    calls: AtomicU64,
    returns: AtomicU64,
}

impl LuaFunctionInstrumentation {
    pub(crate) fn capture(&self, enabled: bool) -> LuaFunctionCapture<'_> {
        if enabled {
            self.calls.store(0, Ordering::Relaxed);
            self.returns.store(0, Ordering::Relaxed);
        }
        self.enabled.store(enabled, Ordering::Relaxed);
        LuaFunctionCapture {
            instrumentation: self,
            enabled,
        }
    }

    pub(crate) fn record_call(&self) {
        if self.enabled.load(Ordering::Relaxed) {
            self.calls.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_return(&self) {
        if self.enabled.load(Ordering::Relaxed) {
            self.returns.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(crate) struct LuaFunctionCapture<'a> {
    instrumentation: &'a LuaFunctionInstrumentation,
    enabled: bool,
}

impl LuaFunctionCapture<'_> {
    pub(crate) fn output(&self) -> Option<LuaFunctionActivity> {
        self.enabled.then(|| LuaFunctionActivity {
            calls: self.instrumentation.calls.load(Ordering::Relaxed),
            returns: self.instrumentation.returns.load(Ordering::Relaxed),
        })
    }
}

impl Drop for LuaFunctionCapture<'_> {
    fn drop(&mut self) {
        self.instrumentation.enabled.store(false, Ordering::Relaxed);
    }
}

/// Machine-local instrumentation selection; it does not control execution.
#[derive(Debug, Clone, Copy)]
pub struct EventCapture {
    pub input_sampled: bool,
    pub frame_completed: bool,
    pub traces: bool,
    /// Count drawing API calls for this completed frame, without retaining calls.
    pub graphics_activity: bool,
    pub execution_activity: bool,
    pub rng_activity: bool,
    /// Count `btn`/`btnp` invocations for this frame without retaining results.
    pub input_api_activity: bool,
    pub lua_function_activity: bool,
}

impl EventCapture {
    pub const NONE: Self = Self {
        input_sampled: false,
        frame_completed: false,
        traces: false,
        graphics_activity: false,
        execution_activity: false,
        rng_activity: false,
        input_api_activity: false,
        lua_function_activity: false,
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameOutcome {
    pub frame: u64,
    pub exit_requested: bool,
    pub events: Vec<Tic80Event>,
    /// None when unrequested; Some(0) when requested but no covered API ran.
    pub graphics_calls: Option<u64>,
    pub execution_activity: Option<ExecutionActivity>,
    pub rng_activity: Option<RngActivity>,
    /// Successful `btn` and `btnp` API invocations during this frame.
    pub input_api_activity: Option<InputApiActivity>,
    pub lua_function_activity: Option<LuaFunctionActivity>,
}

/// Per-frame API-query counts; these do not include returned masks or values.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct InputApiActivity {
    pub btn_calls: u64,
    pub btnp_calls: u64,
}
