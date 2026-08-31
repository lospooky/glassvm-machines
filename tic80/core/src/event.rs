//! Native events produced at machine boundaries.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tic80Event {
    FrameCompleted { frame: u64 },
    InputSampled { mask: u32 },
    Trace { message: String },
}
