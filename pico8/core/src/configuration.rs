//! Native runtime configuration limits.

/// Stable machine identifier shared by the native core and GlassVM bundle.
pub const MACHINE_ID: &str = "pico8";

/// Stable identifier for the native execution semantics implemented by this core.
pub const SEMANTICS: &str = "pico8-0.2x-headless-compat.v2";

/// Smallest safe per-callback instruction budget supported by the runtime.
pub const MIN_INSTRUCTION_BUDGET: u64 = 1_000;

/// Largest artifact accepted by the compatibility runtime.
pub const MAX_ARTIFACT_BYTES: usize = 4 * 1024 * 1024;
