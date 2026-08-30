use crate::configuration::QuirksConfig;
use crate::event::EventLog;
use crate::machine::display::Display;
use crate::machine::font::{BIG_FONT, BIG_FONT_ADDR, FONT, FONT_ADDR};
use crate::machine::keypad::Keypad;
use crate::machine::randomness::Rng;
use crate::snapshot::Snapshot;

pub const ROM_START: u16 = 0x200;
/// XO-CHIP expands address space to 64 kB.
pub const MEM_SIZE: usize = 65536;

/// Maximum ROM size that fits in the expanded 64 kB space.
pub const MAX_ROM_SIZE: usize = MEM_SIZE - ROM_START as usize;

/// Result of a single [`Cpu::tick`] call.
#[derive(Debug, Clone, PartialEq)]
pub enum StepResult {
    /// Instruction executed normally.
    Ok,
    /// CPU was already halted (`00FD` was previously executed).
    Halted,
    /// CPU is waiting for a key press/release (`FX0A`).
    WaitingForKey,
    /// An unknown or unimplemented opcode was encountered.
    InvalidOpcode(u16),
    /// `2NNN` (CALL) with a full stack (16 levels deep).
    StackOverflow,
    /// `00EE` (RET) with an empty stack.
    StackUnderflow,
    /// A memory instruction (FX33/FX55/FX65/5XY2/5XY3) computed an address
    /// beyond the 64 kB address space.  The contained value is the
    /// out-of-bounds byte address that was attempted. This is wider than the
    /// native address register so an access spanning past `0xFFFF` can report
    /// its actual first invalid byte without wrapping the evidence.
    MemoryFault(u32),
}

/// State of the FX0A "wait for key" instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyWait {
    /// Not waiting.
    None,
    /// Waiting for a key to be pressed; will store into V[reg].
    WaitPress(u8),
    /// A key was pressed; now waiting for it to be released.
    WaitRelease(u8, u8),
}

/// The complete CHIP-8/SUPER-CHIP/XO-CHIP interpreter state.
pub struct Cpu {
    /// 64 kB address space (XO-CHIP). Classic ROMs only use the first 4 kB.
    pub mem: [u8; MEM_SIZE],
    /// General-purpose registers V0–VF.
    pub v: [u8; 16],
    /// Index register.
    pub i: u16,
    /// Program counter.
    pub pc: u16,
    /// Stack pointer.
    pub sp: u8,
    /// Call/return stack (16 levels).
    pub stack: [u16; 16],
    /// Delay timer (decrements at 60 Hz).
    pub dt: u8,
    /// Sound timer (decrements at 60 Hz; buzzer while > 0).
    pub st: u8,
    /// Display subsystem.
    pub display: Display,
    /// Keypad state.
    pub keys: Keypad,
    /// Quirks configuration.
    pub quirks: QuirksConfig,
    /// Set to true by `00FD` (SUPER-CHIP exit).
    pub halted: bool,
    /// FX0A key-wait state machine.
    pub key_wait: KeyWait,
    /// Persistent flag registers (FX75 / FX85, SUPER-CHIP).
    pub flags: [u8; 16],
    /// XO-CHIP 16-byte audio pattern buffer.
    pub audio_buf: [u8; 16],
    /// XO-CHIP audio pitch register.
    pub audio_pitch: u8,
    /// Per-instance PRNG (replaces the old thread-local `rng_byte()`).
    pub rng: Rng,
}

impl Cpu {
    pub fn new(quirks: QuirksConfig) -> Self {
        let mut cpu = Self {
            mem: [0; MEM_SIZE],
            v: [0; 16],
            i: 0,
            pc: ROM_START,
            sp: 0,
            stack: [0; 16],
            dt: 0,
            st: 0,
            display: Display::default(),
            keys: Keypad::default(),
            quirks,
            halted: false,
            key_wait: KeyWait::None,
            flags: [0; 16],
            audio_buf: [0; 16],
            audio_pitch: 64,
            rng: Rng::new(0xDEAD_BEEF),
        };
        cpu.load_fonts();
        cpu
    }

    fn load_fonts(&mut self) {
        let fa = FONT_ADDR as usize;
        self.mem[fa..fa + FONT.len()].copy_from_slice(&FONT);
        let bfa = BIG_FONT_ADDR as usize;
        self.mem[bfa..bfa + BIG_FONT.len()].copy_from_slice(&BIG_FONT);
    }

    /// Load a ROM image into memory at `ROM_START`.
    /// Returns `Err` if the ROM is empty or too large.
    pub fn load_rom(&mut self, data: &[u8]) -> Result<(), String> {
        if data.is_empty() {
            return Err("ROM is empty".into());
        }
        if data.len() > MAX_ROM_SIZE {
            return Err(format!(
                "ROM too large: {} bytes (max {})",
                data.len(),
                MAX_ROM_SIZE
            ));
        }
        let start = ROM_START as usize;
        self.mem[start..start + data.len()].copy_from_slice(data);
        self.pc = ROM_START;
        Ok(())
    }

    /// Decrement DT and ST at 60 Hz.  Call once per display frame.
    pub fn tick_timers(&mut self) {
        if self.dt > 0 {
            self.dt -= 1;
        }
        if self.st > 0 {
            self.st -= 1;
        }
    }

    /// Whether the buzzer should be sounding.
    #[inline]
    pub fn buzzer_on(&self) -> bool {
        self.st > 0
    }

    /// Push a value onto the call stack.
    /// Returns `false` and does not modify state if the stack is full.
    pub(crate) fn push(&mut self, val: u16) -> bool {
        if self.sp as usize >= self.stack.len() {
            return false;
        }
        self.stack[self.sp as usize] = val;
        self.sp += 1;
        true
    }

    /// Pop a value from the call stack.
    /// Returns `None` if the stack is empty.
    pub(crate) fn pop(&mut self) -> Option<u16> {
        if self.sp == 0 {
            return None;
        }
        self.sp -= 1;
        Some(self.stack[self.sp as usize])
    }

    /// Fetch the next 16-bit instruction word and advance PC.
    #[inline]
    fn fetch(&mut self) -> u16 {
        let hi = self.mem[self.pc as usize] as u16;
        let lo = self.mem[self.pc.wrapping_add(1) as usize] as u16;
        self.pc = self.pc.wrapping_add(2);
        (hi << 8) | lo
    }

    /// Peek at the opcode at the current PC without advancing it.
    /// Used by the `Engine` to record coverage before dispatching.
    #[inline]
    pub fn fetch_opcode(&self) -> u16 {
        let hi = self.mem[self.pc as usize] as u16;
        let lo = self.mem[self.pc.wrapping_add(1) as usize] as u16;
        (hi << 8) | lo
    }

    /// Execute one instruction. Returns the result of execution.
    ///
    /// Handles the `FX0A` key-wait state machine before dispatching to
    /// [`crate::machine::instruction::execute`]. Events are appended to `events` as
    /// instructions execute.
    pub fn tick(&mut self, events: &mut EventLog) -> StepResult {
        use crate::event::Event;

        if self.halted {
            return StepResult::Halted;
        }

        match self.key_wait {
            KeyWait::WaitPress(reg) => {
                if let Some(key) = self.keys.any_pressed() {
                    self.key_wait = KeyWait::WaitRelease(reg, key);
                }
                return StepResult::WaitingForKey;
            }
            KeyWait::WaitRelease(reg, key) => {
                if !self.keys.is_pressed(key) {
                    self.v[reg as usize] = key;
                    self.key_wait = KeyWait::None;
                    events.push(Event::KeyWaitResolved { key });
                    return StepResult::Ok;
                }
                return StepResult::WaitingForKey;
            }
            KeyWait::None => {}
        }

        let word = self.fetch();
        crate::machine::instruction::execute(self, word, events)
    }

    /// Save a complete snapshot of the current machine state.
    pub fn save_snapshot(&self) -> Snapshot {
        use crate::display::BUF_SIZE;

        let mut mem = Box::new([0u8; MEM_SIZE]);
        mem.copy_from_slice(&self.mem);

        let mut display_buf = Box::new([[0u8; BUF_SIZE]; 2]);
        display_buf[0].copy_from_slice(self.cpu_display_buf(0));
        display_buf[1].copy_from_slice(self.cpu_display_buf(1));

        Snapshot {
            mem,
            v: self.v,
            i: self.i,
            pc: self.pc,
            stack: self.stack,
            sp: self.sp,
            dt: self.dt,
            st: self.st,
            display_buf,
            display_hires: self.display.hires,
            display_plane: self.display.plane,
            keys: self.keys.as_mask(),
            rng_state: self.rng.state(),
            flags: self.flags,
            audio_buf: self.audio_buf,
            audio_pitch: self.audio_pitch,
            halted: self.halted,
            key_wait: self.key_wait,
        }
    }

    /// Restore machine state from a snapshot.
    pub fn load_snapshot(&mut self, snap: &Snapshot) {
        self.mem.copy_from_slice(snap.mem.as_ref());
        self.v = snap.v;
        self.i = snap.i;
        self.pc = snap.pc;
        self.stack = snap.stack;
        self.sp = snap.sp;
        self.dt = snap.dt;
        self.st = snap.st;
        self.display.buf[0].copy_from_slice(&snap.display_buf[0]);
        self.display.buf[1].copy_from_slice(&snap.display_buf[1]);
        self.display.hires = snap.display_hires;
        self.display.plane = snap.display_plane;
        self.keys.from_mask(snap.keys);
        self.rng = crate::machine::randomness::Rng::from_state(snap.rng_state);
        self.flags = snap.flags;
        self.audio_buf = snap.audio_buf;
        self.audio_pitch = snap.audio_pitch;
        self.halted = snap.halted;
        self.key_wait = snap.key_wait;
    }

    fn cpu_display_buf(&self, plane: usize) -> &[u8] {
        &self.display.buf[plane]
    }

    // push() and pop() are defined above with proper error-returning signatures.
}
