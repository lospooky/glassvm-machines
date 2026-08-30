//! Non-executing static reasoning for Hexwell catalyst plates.

pub mod analysis;
pub mod artifact;
pub mod diagnostic;
pub mod report;
pub mod rules;
pub mod structure;
pub mod verification;

pub use analysis::analyze_bytes;
pub use artifact::InspectedArtifact;
pub use diagnostic::{Diagnostic, Severity};
pub use report::{AnalysisReport, VerificationReport};
pub use structure::ArtifactStructure;
pub use verification::verify_bytes;
