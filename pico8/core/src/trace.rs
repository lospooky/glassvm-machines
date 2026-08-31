//! Native runtime evidence extracted without GlassVM types.

use crate::RuntimeSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeCounters {
    pub frame: u64,
    pub draw_calls: u64,
    pub audio_calls: u64,
}

impl From<&RuntimeSnapshot> for RuntimeCounters {
    fn from(snapshot: &RuntimeSnapshot) -> Self {
        Self {
            frame: snapshot.frame,
            draw_calls: snapshot.draw_calls,
            audio_calls: snapshot.audio_calls,
        }
    }
}
