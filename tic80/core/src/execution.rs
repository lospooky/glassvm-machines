//! Native frame execution results.

use crate::event::Tic80Event;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameOutcome {
    pub frame: u64,
    pub exit_requested: bool,
    pub events: Vec<Tic80Event>,
}
