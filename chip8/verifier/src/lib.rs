//! Non-executing analysis and verification for CHIP-8-family ROM artifacts.

pub mod analysis;
pub mod artifact;
pub mod diagnostic;
pub mod report;
pub mod rules;
pub mod structure;
pub mod verification;

pub use analysis::ExtensionLevel;
pub use artifact as loader;
pub use diagnostic::{Diagnostic, Level};
pub use rules::disassembly as disasm;
pub use rules::metrics;
pub use rules::metrics::{BehavioralPriors, StructuralMetrics, ValidityFlags};
pub use structure as cfg;
pub use verification::{AnalysisReport, VerifyReport, analyze_bytes, verify_bytes, verify_file};
