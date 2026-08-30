use std::collections::HashSet;

/// Number of opcode classes (top nibble 0x0–0xF).
pub const OPCODE_CLASS_COUNT: usize = 16;

/// Tracks coverage across a single execution run.
pub struct CoverageTracker {
    /// Set of all executed PC values.
    executed_pcs: HashSet<u16>,
    /// Set of (from_pc, to_pc) control-flow edges.
    edges: HashSet<(u16, u16)>,
    /// Per-class instruction counts (indexed by top nibble).
    opcode_counts: [u32; OPCODE_CLASS_COUNT],
    /// Set of memory addresses written to by store instructions.
    memory_written: HashSet<u16>,
    /// Maximum stack depth reached during the run.
    max_stack_depth: u8,
}

/// A snapshot of coverage data for reporting.
#[derive(Debug, Clone)]
pub struct CoverageSummary {
    pub unique_pcs: usize,
    pub unique_edges: usize,
    pub opcode_counts: [u32; OPCODE_CLASS_COUNT],
    pub memory_written_bytes: usize,
    pub max_stack_depth: u8,
}

impl Default for CoverageSummary {
    fn default() -> Self {
        Self {
            unique_pcs: 0,
            unique_edges: 0,
            opcode_counts: [0; OPCODE_CLASS_COUNT],
            memory_written_bytes: 0,
            max_stack_depth: 0,
        }
    }
}

impl CoverageTracker {
    pub fn new() -> Self {
        Self {
            executed_pcs: HashSet::with_capacity(256),
            edges: HashSet::with_capacity(256),
            opcode_counts: [0; OPCODE_CLASS_COUNT],
            memory_written: HashSet::with_capacity(128),
            max_stack_depth: 0,
        }
    }

    /// Record one executed instruction.
    ///
    /// - `from_pc`: PC value before the instruction was fetched.
    /// - `to_pc`:   PC value after execution (may be a branch target).
    /// - `opcode`:  The raw 16-bit word that was executed.
    pub fn record_step(&mut self, from_pc: u16, to_pc: u16, opcode: u16) {
        self.executed_pcs.insert(from_pc);
        self.edges.insert((from_pc, to_pc));
        let class = (opcode >> 12) as usize;
        self.opcode_counts[class] += 1;
    }

    /// Record a memory write (FX55, FX33, 5XY2, F002 …).
    pub fn record_memory_write(&mut self, addr: u16, len: u16) {
        for offset in 0..len {
            self.memory_written.insert(addr.wrapping_add(offset));
        }
    }

    /// Update the maximum observed stack depth.
    pub fn record_stack_depth(&mut self, sp: u8) {
        if sp > self.max_stack_depth {
            self.max_stack_depth = sp;
        }
    }

    pub fn summary(&self) -> CoverageSummary {
        CoverageSummary {
            unique_pcs: self.executed_pcs.len(),
            unique_edges: self.edges.len(),
            opcode_counts: self.opcode_counts,
            memory_written_bytes: self.memory_written.len(),
            max_stack_depth: self.max_stack_depth,
        }
    }

    pub fn reset(&mut self) {
        self.executed_pcs.clear();
        self.edges.clear();
        self.opcode_counts = [0; OPCODE_CLASS_COUNT];
        self.memory_written.clear();
        self.max_stack_depth = 0;
    }
}

impl Default for CoverageTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_unique_pcs_and_edges() {
        let mut cov = CoverageTracker::new();
        cov.record_step(0x200, 0x202, 0x1202);
        cov.record_step(0x202, 0x204, 0x6000);
        cov.record_step(0x200, 0x202, 0x1202); // duplicate
        let s = cov.summary();
        assert_eq!(s.unique_pcs, 2);
        assert_eq!(s.unique_edges, 2);
    }

    #[test]
    fn opcode_class_counts() {
        let mut cov = CoverageTracker::new();
        cov.record_step(0x200, 0x202, 0x6A00); // class 6
        cov.record_step(0x202, 0x204, 0x6B00); // class 6
        cov.record_step(0x204, 0x206, 0xD015); // class D
        let s = cov.summary();
        assert_eq!(s.opcode_counts[0x6], 2);
        assert_eq!(s.opcode_counts[0xD], 1);
        assert_eq!(s.opcode_counts[0x1], 0);
    }

    #[test]
    fn memory_write_tracking() {
        let mut cov = CoverageTracker::new();
        cov.record_memory_write(0x300, 3); // addresses 0x300, 0x301, 0x302
        cov.record_memory_write(0x301, 1); // duplicate
        let s = cov.summary();
        assert_eq!(s.memory_written_bytes, 3);
    }

    #[test]
    fn max_stack_depth() {
        let mut cov = CoverageTracker::new();
        cov.record_stack_depth(2);
        cov.record_stack_depth(5);
        cov.record_stack_depth(3);
        assert_eq!(cov.summary().max_stack_depth, 5);
    }

    #[test]
    fn reset_clears_all() {
        let mut cov = CoverageTracker::new();
        cov.record_step(0x200, 0x202, 0x6000);
        cov.record_memory_write(0x300, 4);
        cov.record_stack_depth(3);
        cov.reset();
        let s = cov.summary();
        assert_eq!(s.unique_pcs, 0);
        assert_eq!(s.memory_written_bytes, 0);
        assert_eq!(s.max_stack_depth, 0);
    }
}
