//! GlassVM adapters for Hexwell evidence and native static reports.

mod native_events;
mod static_analyzer;
mod verifier;

pub use native_events::HexwellNativeEventAdapter;
pub use static_analyzer::HexwellStaticAnalyzerBackend;
pub use verifier::HexwellVerifierBackend;
