//! Native output views.

use crate::{DISPLAY_HEIGHT, DISPLAY_WIDTH, MachineState};

/// Borrowed indexed-color canvas output.
#[derive(Debug, Clone, Copy)]
pub struct CanvasView<'a> {
    pub width: usize,
    pub height: usize,
    pub pixels: &'a [u8],
    pub palette: &'a [u8; 16],
}

impl<'a> From<&'a MachineState> for CanvasView<'a> {
    fn from(state: &'a MachineState) -> Self {
        Self {
            width: DISPLAY_WIDTH,
            height: DISPLAY_HEIGHT,
            pixels: &state.canvas,
            palette: &state.palette,
        }
    }
}
