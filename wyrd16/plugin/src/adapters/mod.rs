//! GlassVM adapters for native Wyrd-16 evidence and static reports.

mod native_events;
mod static_analyzer;
mod verifier;

pub use native_events::Wyrd16NativeEventAdapter;
pub use static_analyzer::Wyrd16StaticAnalyzerBackend;
pub use verifier::Wyrd16VerifierBackend;
