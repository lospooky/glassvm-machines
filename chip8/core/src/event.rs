use crate::trace::{MemoryChange, MemoryRead};

/// Typed events emitted during instruction execution.
///
/// The [`crate::emulator::Engine`] collects these in an [`EventLog`] during a
/// run. Events can be inspected after execution to build summaries, detect
/// anomalies, or feed replay systems.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// `00E0` — screen cleared (selected bitplanes only).
    ClearScreen,
    /// `1NNN` or `BNNN` — unconditional jump.
    Jump { from: u16, to: u16 },
    /// `2NNN` — subroutine call.
    Call { from: u16, to: u16 },
    /// `00EE` — subroutine return.
    Return { to: u16 },
    /// `DXYN` — sprite drawn; `collision` is true when any lit pixel was
    /// turned off by the XOR.
    Draw {
        x: u8,
        y: u8,
        n: u8,
        collision: bool,
    },
    /// `FX0A` entered the key-wait state.
    KeyWaitEntered,
    /// Key wait resolved: key was pressed and then released.
    KeyWaitResolved { key: u8 },
    /// `FX15` / `FX18` — a timer was loaded.
    TimerSet { timer: Timer, value: u8 },
    /// `FX55` / `FX33` / `5XY2` — bytes written to memory at `addr`.
    MemoryWrite { addr: u16, len: u16 },
    /// `2NNN` pushed a return address (stack depth increased).
    StackPush,
    /// `00EE` popped a return address (stack depth decreased).
    StackPop,
    /// `00CN` — display scrolled down by `n` rows.
    ScrollDown { n: u8 },
    /// `00DN` — display scrolled up by `n` rows (XO-CHIP).
    ScrollUp { n: u8 },
    /// `00FB` — display scrolled right by 4 pixels.
    ScrollRight,
    /// `00FC` — display scrolled left by 4 pixels.
    ScrollLeft,
    /// `00FF` — hires mode (128×64) enabled.
    HiresEnabled,
    /// `00FE` — lores mode (64×32) enabled.
    LoresEnabled,
    /// Unknown / unimplemented opcode encountered.
    InvalidOpcode { pc: u16, opcode: u16 },
}

/// Which timer was set by a `TimerSet` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    Delay,
    Sound,
}

/// A log of [`Event`]s accumulated during execution.
///
/// Owned by [`crate::emulator::Engine`]; passed by `&mut` reference into
/// instruction execution so handlers can append events without any extra
/// allocation on the happy path.
#[derive(Default)]
pub struct EventLog {
    events: Vec<Event>,
    memory_capture: Option<MemoryCapture>,
}

#[derive(Default)]
struct MemoryCapture {
    reads: Vec<MemoryRead>,
    changes: Vec<MemoryChange>,
}

impl EventLog {
    pub fn new() -> Self {
        Self {
            events: Vec::with_capacity(4096),
            memory_capture: None,
        }
    }

    #[inline]
    pub fn push(&mut self, event: Event) {
        self.events.push(event);
    }

    /// Start exact memory-access capture for one observed engine step.
    ///
    /// This is crate-internal so direct CPU users retain the ordinary event-only
    /// path. No capture buffer exists while detailed step recording is off.
    pub(crate) fn begin_memory_capture(&mut self) {
        debug_assert!(self.memory_capture.is_none());
        self.memory_capture = Some(MemoryCapture::default());
    }

    /// Clone pre-write bytes only when an observed engine step is active.
    #[inline]
    pub(crate) fn capture_memory_before(&self, bytes: &[u8]) -> Option<Vec<u8>> {
        self.memory_capture.as_ref().map(|_| bytes.to_vec())
    }

    /// Record an exact native memory read only when detailed capture is active.
    #[inline]
    pub(crate) fn record_memory_read(&mut self, address: u16, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if let Some(capture) = self.memory_capture.as_mut() {
            capture.reads.push(MemoryRead {
                address,
                bytes: bytes.to_vec(),
            });
        }
    }

    /// Emit the ordinary native write event and, when requested, retain its
    /// exact before/after bytes for the enclosing step record.
    #[inline]
    pub(crate) fn push_memory_write(
        &mut self,
        address: u16,
        before: Option<Vec<u8>>,
        after: &[u8],
    ) {
        let len = u16::try_from(after.len()).expect("CHIP-8 memory write length fits in u16");
        self.events.push(Event::MemoryWrite { addr: address, len });
        match (&mut self.memory_capture, before) {
            (Some(capture), Some(before)) => capture.changes.push(MemoryChange {
                address,
                before,
                after: after.to_vec(),
            }),
            (Some(_), None) => {
                panic!("observed CHIP-8 memory write is missing its before bytes")
            }
            (None, before) => debug_assert!(before.is_none()),
        }
    }

    /// Finish one observed step and return its exact memory reads and writes.
    pub(crate) fn finish_memory_capture(&mut self) -> (Vec<MemoryRead>, Vec<MemoryChange>) {
        let capture = self
            .memory_capture
            .take()
            .expect("memory capture must be active for an observed step");
        (capture.reads, capture.changes)
    }

    /// Drain all accumulated events, returning them as a `Vec` and clearing
    /// the log.
    pub fn drain(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn as_slice(&self) -> &[Event] {
        &self.events
    }

    pub fn clear(&mut self) {
        self.events.clear();
        self.memory_capture = None;
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}
