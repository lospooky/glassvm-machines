//! Native runtime event categories.

/// Events established directly by the native runtime before normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeEventKind {
    FrameCompleted,
    InputSampled,
    AudioCommand,
    DisplayWrite,
    RuntimeError,
}
