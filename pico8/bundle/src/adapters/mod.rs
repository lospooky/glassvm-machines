mod static_analyzer;
mod verifier;

pub use static_analyzer::Pico8StaticAnalyzerBackend;
pub use verifier::Pico8VerifierBackend;

pub(crate) use static_analyzer::analysis_capabilities;
