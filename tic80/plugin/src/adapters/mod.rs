mod static_analyzer;
mod verifier;

pub use static_analyzer::Tic80StaticAnalyzerBackend;
pub use verifier::Tic80VerifierBackend;

pub(crate) use static_analyzer::analysis_capabilities;
