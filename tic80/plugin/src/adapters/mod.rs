//! GlassVM normalization and static-tool adapters.

mod native_events;
mod observables;
mod static_analyzer;
mod verifier;

pub use native_events::Tic80NativeEventAdapter;
pub use observables::Tic80ObservableAdapter;
pub use static_analyzer::Tic80StaticAnalyzer;
pub use verifier::Tic80Verifier;
