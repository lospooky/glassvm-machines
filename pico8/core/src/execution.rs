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
    pub rng: Option<RngActivity>,
}
