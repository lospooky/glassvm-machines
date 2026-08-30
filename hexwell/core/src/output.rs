//! Native reactor output view.

use crate::ReactorState;

pub struct ReactorOutput<'a> {
    pub state: &'a ReactorState,
}

impl<'a> From<&'a ReactorState> for ReactorOutput<'a> {
    fn from(state: &'a ReactorState) -> Self {
        Self { state }
    }
}
