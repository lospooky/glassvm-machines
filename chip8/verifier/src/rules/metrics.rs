/// Structured output sub-objects emitted by the extended analysis passes.
///
/// All three structs derive `Default` so callers get zero/false values when a
/// pass is skipped (e.g. because an earlier fatal error aborted the pipeline).
use serde::Serialize;

// ---------------------------------------------------------------------------
// ValidityFlags
// ---------------------------------------------------------------------------

/// Hard validity properties of the ROM.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ValidityFlags {
    /// Ratio of known-valid opcode words to total ROM words (full linear
    /// disassembly, not just reachable code).  Range 0.0 – 1.0.
    pub valid_opcode_ratio: f64,

    /// `true` if any reachable instruction decoded as `OpKind::Unknown`
    /// (diagnostic E004 was emitted).
    pub has_illegal_opcode: bool,

    /// `true` when the CFG contains no out-of-bounds or misaligned edges
    /// (no E003, E005, E006, E007 diagnostics).
    pub cfg_well_formed: bool,

    /// `true` when a `RET` (00EE) instruction is reachable along at least one
    /// path that passes through no `CALL` — the return would underflow the
    /// hardware stack.
    pub stack_underflow_risk: bool,

    /// `true` when the static call graph reaches a depth greater than 12
    /// (original COSMAC VIP stack limit) or contains a recursive cycle.
    pub stack_overflow_risk: bool,

    /// Number of out-of-bounds or misaligned CFG edges (sum of E003 + E005 +
    /// E006 + E007 diagnostics).
    pub out_of_bounds_jump_count: u32,

    /// Number of statically-traceable `LD I, addr` → mem-dereference pairs
    /// where the computed address + access width exceeds the ROM region or the
    /// full 4 kB address space.
    pub out_of_bounds_mem_ref_count: u32,

    /// `true` when at least one reachable loop body (a basic block that
    /// contains a backward branch) contains an instruction with a visible
    /// side effect: `Draw`, `Cls`, `SetDelay`, `SetSound`, `Rand`, `Audio`,
    /// or `Pitch`.  Used by the `S_dynamic` scoring component.
    pub has_side_effect_in_loop: bool,

    /// `true` when at least one conditional skip instruction
    /// (`3xNN`, `4xNN`, `5xy0`, `9xy0`, `Ex9E`, `ExA1`) is reachable.
    /// Used as the soft-validity criterion S5.
    pub has_any_skip: bool,
}

// ---------------------------------------------------------------------------
// StructuralMetrics
// ---------------------------------------------------------------------------

/// Shape of the reachable control-flow graph.
#[derive(Debug, Default, Clone, Serialize)]
pub struct StructuralMetrics {
    /// Number of instructions reachable from the entry point (`0x200`).
    pub reachable_instruction_count: usize,

    /// Number of basic blocks in the reachable CFG.
    pub basic_block_count: usize,

    /// Number of back edges found during DFS on the basic-block graph (a
    /// proxy for the number of loops).
    pub loop_count: usize,

    /// Longest path from the entry block to any exit, measured in basic blocks
    /// and computed on the DAG obtained by removing back edges.
    pub max_cfg_depth: usize,

    /// `reachable_instruction_count / total_instruction_slots`.  Range 0.0 – 1.0.
    pub reachable_ratio: f64,

    /// Sum of `byte_len` for all reachable instructions.  Equal to
    /// `reachable_instruction_count * 2` unless XO-CHIP long-load (`F000 NNNN`)
    /// instructions are present (those are 4 bytes each).
    pub estimated_code_bytes: usize,

    /// `rom_len - estimated_code_bytes`.  Bytes not accounted for by reachable
    /// instructions are assumed to be data (sprites, tables, strings, …).
    pub estimated_data_bytes: usize,
}

// ---------------------------------------------------------------------------
// BehavioralPriors
// ---------------------------------------------------------------------------

/// High-level behavioural capabilities inferred from reachable opcodes.
#[derive(Debug, Default, Clone, Serialize)]
pub struct BehavioralPriors {
    /// Any `DRW Vx, Vy, n` (Dxyn) instruction is reachable.
    pub contains_draw: bool,

    /// Any `SKP`, `SKNP`, or `LD Vx, K` (wait-for-key) instruction is
    /// reachable.
    pub contains_key_input: bool,

    /// Any `LD DT, Vx` (Fx15) or `LD Vx, DT` (Fx07) instruction is reachable.
    pub contains_timers: bool,

    /// Any `LD ST, Vx` (Fx18), `pitch Vx` (Fx3A), or `audio` (F002)
    /// instruction is reachable.
    pub contains_sound: bool,

    /// A `DRW` instruction is immediately followed (within the same basic
    /// block) by an instruction that reads VF — the idiomatic collision-check
    /// pattern.
    pub contains_collision_detection: bool,

    /// Any `RND Vx, byte` (Cxnn) instruction is reachable.
    pub contains_randomness: bool,
}
