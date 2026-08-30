//! Native Hexwell identity and boot configuration.

pub const MACHINE_ID: &str = "hexwell";
pub const SEMANTICS: &str = "hexwell-semantics.v1";
pub const MACHINE_VERSION: &str = SEMANTICS;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MachineConfiguration {
    pub seed: u64,
}
