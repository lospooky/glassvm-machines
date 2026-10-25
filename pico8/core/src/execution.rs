//! Native frame execution surface.

pub use crate::machine::lua_runtime::Pico8Runtime;

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
    pub update_calls: u64,
    pub draw_calls: u64,
    pub instruction_hook_ticks: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FrameActivity {
    pub execution: Option<ExecutionActivity>,
    pub lua_functions: Option<LuaFunctionActivity>,
    pub rng: Option<RngActivity>,
    pub input_api: Option<InputApiActivity>,
}

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

/// Successful calls to the PICO-8 input-query APIs during one completed frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct InputApiActivity {
    pub btn_calls: u64,
    pub btnp_calls: u64,
}

#[derive(Default)]
pub(crate) struct InputApiInstrumentation {
    enabled: AtomicBool,
    btn_calls: AtomicU64,
    btnp_calls: AtomicU64,
}

impl InputApiInstrumentation {
    pub(crate) fn capture(&self, enabled: bool) -> InputApiCapture<'_> {
        if enabled {
            self.btn_calls.store(0, Ordering::Relaxed);
            self.btnp_calls.store(0, Ordering::Relaxed);
        }
        self.enabled.store(enabled, Ordering::Relaxed);
        InputApiCapture {
            instrumentation: self,
            enabled,
        }
    }

    pub(crate) fn record_btn(&self) {
        if self.enabled.load(Ordering::Relaxed) {
            self.btn_calls.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_btnp(&self) {
        if self.enabled.load(Ordering::Relaxed) {
            self.btnp_calls.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(crate) struct InputApiCapture<'a> {
    instrumentation: &'a InputApiInstrumentation,
    enabled: bool,
}

impl InputApiCapture<'_> {
    pub(crate) fn output(&self) -> Option<InputApiActivity> {
        self.enabled.then(|| InputApiActivity {
            btn_calls: self.instrumentation.btn_calls.load(Ordering::Relaxed),
            btnp_calls: self.instrumentation.btnp_calls.load(Ordering::Relaxed),
        })
    }
}

impl Drop for InputApiCapture<'_> {
    fn drop(&mut self) {
        self.instrumentation.enabled.store(false, Ordering::Relaxed);
    }
}
