//! Native diagnostic trace output.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceMessage {
    pub frame: u64,
    pub message: String,
}
