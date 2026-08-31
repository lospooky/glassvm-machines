//! Non-executing analysis and verification for TIC-80 cartridges.

pub mod analysis;
pub mod artifact;
pub mod diagnostic;
pub mod report;
pub mod rules;
pub mod structure;
pub mod verification;

pub use analysis::analyze_bytes;
pub use diagnostic::{Diagnostic, Severity};
pub use report::{AnalysisReport, VerificationReport};
pub use verification::verify_bytes;
