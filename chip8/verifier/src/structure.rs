/// Stage 2 — Control Flow Graph Builder
///
/// Performs a worklist-based reachability analysis from 0x200 to determine
/// which instructions are executable vs. probable data.
/// After reachability, partitions the reachable code into basic blocks and
/// runs: DFS back-edge counting (loops), DAG longest-path (max depth), and
/// call-graph depth / recursion detection.
use std::collections::{HashMap, HashSet};

use crate::diagnostic::Diagnostic;
use crate::disasm::{Instruction, OpKind};
use crate::loader::ROM_BASE;

// ---------------------------------------------------------------------------
// Basic block
// ---------------------------------------------------------------------------

/// A maximal straight-line sequence of instructions with a single entry and
/// (possibly multiple) exits.
#[derive(Debug, Clone)]
pub struct BasicBlock {
    /// Address of the first instruction in this block.
    pub leader: u16,
    /// Addresses of all instructions in the block, in order.
    pub addrs: Vec<u16>,
    /// Addresses of successor blocks (jump/fall-through targets).
    pub successors: Vec<u16>,
}

// ---------------------------------------------------------------------------
// CFG
// ---------------------------------------------------------------------------

pub struct Cfg {
    /// Map from address → decoded instruction for all reachable instructions.
    pub reachable: HashMap<u16, Instruction>,
    /// Addresses that are reachable but are targets of indirect jumps (jump0).
    pub indirect_jump_sites: Vec<u16>,
    /// Addresses used as call-return targets (addr after each `call`).
    pub return_sites: HashSet<u16>,
    /// Diagnostics produced during CFG construction.
    pub diags: Vec<Diagnostic>,

    // ---- Basic-block partition (populated by build()) ---------------------
    /// All basic blocks, keyed by their leader address.
    pub basic_blocks: HashMap<u16, BasicBlock>,

    // ---- CFG DFS metrics --------------------------------------------------
    /// Number of back edges in the DFS on the basic-block graph (loop count).
    pub loop_count: usize,
    /// Longest acyclic path from the entry block, measured in basic blocks.
    pub max_cfg_depth: usize,

    // ---- Call graph metrics -----------------------------------------------
    /// Edges in the static call graph: `(caller_addr, callee_addr)`.
    pub call_graph_edges: Vec<(u16, u16)>,
    /// Maximum observed call-chain depth.
    pub max_call_depth: usize,
    /// True if the call graph contains a cycle (recursive call).
    pub has_recursive_call: bool,
}

/// Build a CFG by reachability from the ROM entry point.
///
/// `instrs` is the flat list from the disassembler, indexed by their address.
pub fn build(
    instrs: &[Instruction],
    rom_end: usize, // exclusive upper bound; may be the 0x10000 sentinel
) -> Cfg {
    // Build an address → instruction map for fast lookup.
    let by_addr: HashMap<u16, &Instruction> = instrs.iter().map(|i| (i.addr, i)).collect();

    let mut reachable: HashMap<u16, Instruction> = HashMap::new();
    let mut return_sites: HashSet<u16> = HashSet::new();
    let mut indirect_jump_sites: Vec<u16> = Vec::new();
    let mut diags: Vec<Diagnostic> = Vec::new();
    let mut call_graph_edges: Vec<(u16, u16)> = Vec::new();

    // Return-address stack: each call pushes (call_site_return_addr).
    // We track a *set* of possible return addresses (conservative: union over all paths).
    let mut pending_returns: HashSet<u16> = HashSet::new();

    let mut worklist: Vec<u16> = vec![ROM_BASE as u16];
    let mut visited: HashSet<u16> = HashSet::new();
    // A return discovered before a later call must still make that call's
    // continuation reachable.  Without this fixed-point bit, traversal order
    // (including the randomized iteration order of `HashSet`) could decide
    // whether a return site was explored at all.
    let mut has_reachable_ret = false;

    while let Some(addr) = worklist.pop() {
        if visited.contains(&addr) {
            continue;
        }

        // Validate the address.
        if !is_valid_addr(addr, rom_end) {
            diags.push(Diagnostic::error(
                "E003",
                addr,
                format!("CFG edge to invalid address 0x{addr:03X} (out of ROM bounds or odd)"),
            ));
            continue;
        }

        let Some(instr) = by_addr.get(&addr) else {
            // Address is in range but wasn't disassembled (e.g. inside a 4-byte SetILong).
            // Treat as unknown data.
            continue;
        };

        visited.insert(addr);
        reachable.insert(addr, (*instr).clone());

        // Compute successors.
        match instr.kind {
            // Unconditional terminals: no fall-through.
            OpKind::Ret => {
                has_reachable_ret = true;
                // Add all pending return sites — conservative overapproximation.
                let mut return_addrs: Vec<_> = pending_returns.iter().copied().collect();
                return_addrs.sort_unstable();
                for ret_addr in return_addrs {
                    worklist.push(ret_addr);
                }
            }
            OpKind::Exit => {}
            OpKind::Jump => {
                let target = instr.nnn;
                if target < ROM_BASE as u16 {
                    diags.push(Diagnostic::error(
                        "E005",
                        addr,
                        format!(
                            "Jump target 0x{target:03X} is in reserved area (< 0x{ROM_BASE:03X})"
                        ),
                    ));
                } else if target as usize >= rom_end {
                    diags.push(Diagnostic::error(
                        "E006",
                        addr,
                        format!("Jump target 0x{target:03X} is beyond ROM end (0x{rom_end:03X})"),
                    ));
                } else if target % 2 != 0 {
                    diags.push(Diagnostic::error(
                        "E007",
                        addr,
                        format!("Jump target 0x{target:03X} is misaligned (odd address)"),
                    ));
                } else {
                    worklist.push(target);
                }
            }
            OpKind::Call => {
                let target = instr.nnn;
                call_graph_edges.push((addr, target));

                let ret_addr = instr.next_addr();
                if ret_addr < rom_end {
                    let ret_addr = ret_addr as u16;
                    return_sites.insert(ret_addr);
                    let new_return_site = pending_returns.insert(ret_addr);

                    // If any RET was already reachable, close the other half
                    // of the conservative call/return relation immediately.
                    if new_return_site && has_reachable_ret {
                        worklist.push(ret_addr);
                    }
                }

                if target < ROM_BASE as u16 {
                    diags.push(Diagnostic::error(
                        "E005",
                        addr,
                        format!(
                            "Call target 0x{target:03X} is in reserved area (< 0x{ROM_BASE:03X})"
                        ),
                    ));
                    // Still add fall-through as reachable (if call is skipped somehow).
                } else if target as usize >= rom_end {
                    diags.push(Diagnostic::error(
                        "E006",
                        addr,
                        format!("Call target 0x{target:03X} is beyond ROM end (0x{rom_end:03X})"),
                    ));
                } else if target % 2 != 0 {
                    diags.push(Diagnostic::error(
                        "E007",
                        addr,
                        format!("Call target 0x{target:03X} is misaligned (odd address)"),
                    ));
                } else {
                    worklist.push(target);
                }
            }
            OpKind::JumpOffset => {
                // Indirect jump: target = NNN + V0 (or VX on SCHIP). Unresolvable statically.
                diags.push(Diagnostic::info(
                    "W005",
                    addr,
                    format!(
                        "Indirect jump (jump0) at 0x{addr:03X}: target region not statically resolvable"
                    ),
                ));
                indirect_jump_sites.push(addr);
                // Fall-through is NOT added — this is a true jump.
            }

            // Conditional skips: two successors — addr+2 (not taken) and addr+4 (taken).
            kind if kind.is_skip() => {
                let not_taken = instr.next_addr(); // addr + 2
                let taken = not_taken + 2; // addr + 4 (skips one instruction)
                if not_taken < rom_end {
                    worklist.push(not_taken as u16);
                }
                if taken < rom_end {
                    worklist.push(taken as u16);
                }
            }

            // Everything else falls through to addr + byte_len.
            _ => {
                let next = instr.next_addr();
                if next < rom_end {
                    worklist.push(next as u16);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Partition reachable instructions into basic blocks.
    // -----------------------------------------------------------------------
    let basic_blocks = build_basic_blocks(&reachable, &return_sites, rom_end);

    // -----------------------------------------------------------------------
    // DFS on the basic-block graph: count back edges (loops) and find the
    // longest acyclic path from the entry block (max CFG depth).
    // -----------------------------------------------------------------------
    let (loop_count, max_cfg_depth) = cfg_dfs(&basic_blocks);

    // -----------------------------------------------------------------------
    // Call-graph analysis: max depth and recursion detection.
    // -----------------------------------------------------------------------
    let (max_call_depth, has_recursive_call) = call_graph_analysis(&call_graph_edges);

    Cfg {
        reachable,
        indirect_jump_sites,
        return_sites,
        diags,
        basic_blocks,
        loop_count,
        max_cfg_depth,
        call_graph_edges,
        max_call_depth,
        has_recursive_call,
    }
}

// ---------------------------------------------------------------------------
// Basic-block partitioning
// ---------------------------------------------------------------------------

/// Partition the reachable instruction set into basic blocks.
///
/// A *leader* is the first instruction of a basic block.  Leaders are:
///   - The entry point (0x200)
///   - Every jump / call target that is reachable
///   - Every return site (instruction after a call)
///   - Every fall-through after a skip instruction (addr+2)
///   - Every skip-target   (addr+4)
///
/// Once leaders are known, build each block by walking forward from its
/// leader until we hit another leader or a terminal instruction.
fn build_basic_blocks(
    reachable: &HashMap<u16, Instruction>,
    return_sites: &HashSet<u16>,
    rom_end: usize,
) -> HashMap<u16, BasicBlock> {
    // ---- 1. Collect leaders -----------------------------------------------
    let mut leaders: HashSet<u16> = HashSet::new();
    leaders.insert(ROM_BASE as u16); // entry point is always a leader

    for instr in reachable.values() {
        match instr.kind {
            OpKind::Jump => {
                leaders.insert(instr.nnn);
            }
            OpKind::Call => {
                leaders.insert(instr.nnn); // callee entry
            }
            kind if kind.is_skip() => {
                let not_taken = instr.next_addr();
                let taken = not_taken + 2;
                if not_taken < rom_end {
                    leaders.insert(not_taken as u16);
                }
                if taken < rom_end {
                    leaders.insert(taken as u16);
                }
            }
            _ => {}
        }
    }
    // Return sites are also leaders (instruction after a call resuming execution).
    for &rs in return_sites {
        leaders.insert(rs);
    }

    // Keep only leaders that are actually in the reachable set.
    let leaders: HashSet<u16> = leaders
        .into_iter()
        .filter(|a| reachable.contains_key(a))
        .collect();

    // ---- 2. Sort all reachable addresses for sequential walk ---------------
    let mut sorted_addrs: Vec<u16> = reachable.keys().copied().collect();
    sorted_addrs.sort_unstable();

    // ---- 3. Build each basic block -----------------------------------------
    let mut blocks: HashMap<u16, BasicBlock> = HashMap::new();

    for &leader in &leaders {
        let mut addrs: Vec<u16> = Vec::new();
        let mut successors: Vec<u16> = Vec::new();

        // Walk forward from the leader.
        let start_idx = match sorted_addrs.binary_search(&leader) {
            Ok(i) => i,
            Err(_) => continue, // leader not in sorted list (shouldn't happen)
        };

        for &addr in &sorted_addrs[start_idx..] {
            // Stop if we hit a different leader (after the first instruction).
            if !addrs.is_empty() && leaders.contains(&addr) {
                // Fall-through into the next leader.
                successors.push(addr);
                break;
            }

            addrs.push(addr);

            let instr = &reachable[&addr];
            match instr.kind {
                OpKind::Ret | OpKind::Exit | OpKind::JumpOffset => {
                    // Terminal: no explicit successor.
                    break;
                }
                OpKind::Jump => {
                    if reachable.contains_key(&instr.nnn) {
                        successors.push(instr.nnn);
                    }
                    break;
                }
                OpKind::Call => {
                    // Call: successor is the return-address (fall-through after call).
                    let ret = instr.next_addr();
                    if ret <= u16::MAX as usize && reachable.contains_key(&(ret as u16)) {
                        successors.push(ret as u16);
                    }
                    // Also add callee entry as a successor so the BB graph is
                    // connected across procedure boundaries.
                    if reachable.contains_key(&instr.nnn) {
                        successors.push(instr.nnn);
                    }
                    break;
                }
                kind if kind.is_skip() => {
                    let not_taken = instr.next_addr();
                    let taken = not_taken + 2;
                    if not_taken <= u16::MAX as usize && reachable.contains_key(&(not_taken as u16))
                    {
                        successors.push(not_taken as u16);
                    }
                    if taken <= u16::MAX as usize && reachable.contains_key(&(taken as u16)) {
                        successors.push(taken as u16);
                    }
                    break;
                }
                _ => {
                    // Normal fall-through — continue loop; termination is handled
                    // by the leader-boundary check at the top of the loop.
                }
            }
        }

        if !addrs.is_empty() {
            // Deduplicate successors while preserving order.
            successors.dedup();
            blocks.insert(
                leader,
                BasicBlock {
                    leader,
                    addrs,
                    successors,
                },
            );
        }
    }

    blocks
}

// ---------------------------------------------------------------------------
// CFG DFS: back-edge count (loops) and longest DAG path (max depth)
// ---------------------------------------------------------------------------

/// DFS on the basic-block graph.
/// Returns `(loop_count, max_cfg_depth)`.
fn cfg_dfs(blocks: &HashMap<u16, BasicBlock>) -> (usize, usize) {
    if blocks.is_empty() {
        return (0, 0);
    }

    let entry = ROM_BASE as u16;
    let mut loop_count = 0usize;
    // DFS colour: 0 = white, 1 = grey (on stack), 2 = black (done).
    let mut colour: HashMap<u16, u8> = HashMap::new();
    // Depth of the entry in the longest-path DAG (memoised).
    let mut depth_cache: HashMap<u16, usize> = HashMap::new();

    // Iterative DFS for back-edge detection.
    // We need a proper recursive DFS over a potentially large graph; iterative
    // with an explicit stack that records "pop phase".
    struct Frame {
        addr: u16,
        child_idx: usize,
    }

    let mut stack: Vec<Frame> = Vec::new();
    if blocks.contains_key(&entry) {
        colour.insert(entry, 1);
        stack.push(Frame {
            addr: entry,
            child_idx: 0,
        });
    }

    while let Some(frame) = stack.last_mut() {
        let addr = frame.addr;
        let succs = blocks
            .get(&addr)
            .map(|b| b.successors.as_slice())
            .unwrap_or(&[]);

        if frame.child_idx < succs.len() {
            let child = succs[frame.child_idx];
            frame.child_idx += 1;

            // Only visit nodes that are actual blocks.
            if !blocks.contains_key(&child) {
                continue;
            }

            match colour.get(&child).copied().unwrap_or(0) {
                1 => {
                    // Back edge → loop.
                    loop_count += 1;
                }
                0 => {
                    colour.insert(child, 1);
                    stack.push(Frame {
                        addr: child,
                        child_idx: 0,
                    });
                }
                _ => {} // already finished
            }
        } else {
            // All children processed: finish this node.
            colour.insert(addr, 2);
            stack.pop();
        }
    }

    // Longest path from entry on the DAG (ignoring back edges) via memoised DFS.
    fn longest_path(
        addr: u16,
        blocks: &HashMap<u16, BasicBlock>,
        cache: &mut HashMap<u16, usize>,
        visiting: &mut HashSet<u16>,
    ) -> usize {
        if let Some(&d) = cache.get(&addr) {
            return d;
        }
        if visiting.contains(&addr) {
            // Back edge: return 0 to break cycle.
            return 0;
        }
        let Some(block) = blocks.get(&addr) else {
            return 0;
        };
        visiting.insert(addr);
        let max_child = block
            .successors
            .iter()
            .map(|&s| longest_path(s, blocks, cache, visiting))
            .max()
            .unwrap_or(0);
        visiting.remove(&addr);
        let result = 1 + max_child;
        cache.insert(addr, result);
        result
    }

    let mut visiting: HashSet<u16> = HashSet::new();
    let max_depth = if blocks.contains_key(&entry) {
        longest_path(entry, blocks, &mut depth_cache, &mut visiting)
    } else {
        0
    };

    (loop_count, max_depth)
}

// ---------------------------------------------------------------------------
// Call-graph analysis
// ---------------------------------------------------------------------------

/// Analyse the static call graph.
/// Returns `(max_call_depth, has_recursive_call)`.
fn call_graph_analysis(edges: &[(u16, u16)]) -> (usize, bool) {
    if edges.is_empty() {
        return (0, false);
    }

    // Build adjacency list: caller → list of callees.
    let mut adj: HashMap<u16, Vec<u16>> = HashMap::new();
    let mut all_nodes: HashSet<u16> = HashSet::new();
    for &(caller, callee) in edges {
        adj.entry(caller).or_default().push(callee);
        all_nodes.insert(caller);
        all_nodes.insert(callee);
    }

    for children in adj.values_mut() {
        children.sort_unstable();
        children.dedup();
    }
    let mut starts: Vec<_> = all_nodes.into_iter().collect();
    starts.sort_unstable();

    let mut has_recursive_call = false;
    let mut max_depth = 0usize;

    // Every call-site address has at most one callee, so an independent DFS
    // from every node is bounded by O(V^2).  Do not share a global visited set:
    // starting in the middle of a chain first would otherwise suppress the
    // longer traversal from its true upstream root.
    for start in starts {
        struct CFrame {
            addr: u16,
            child_idx: usize,
            depth: usize,
        }

        let mut stack: Vec<CFrame> = Vec::new();
        let mut on_stack: HashSet<u16> = HashSet::new();

        stack.push(CFrame {
            addr: start,
            child_idx: 0,
            depth: 1,
        });
        on_stack.insert(start);

        while let Some(frame) = stack.last_mut() {
            let addr = frame.addr;
            let depth = frame.depth;
            if depth > max_depth {
                max_depth = depth;
            }

            let children = adj.get(&addr).map(|v| v.as_slice()).unwrap_or(&[]);
            if frame.child_idx < children.len() {
                let child = children[frame.child_idx];
                frame.child_idx += 1;

                if on_stack.contains(&child) {
                    has_recursive_call = true;
                } else {
                    on_stack.insert(child);
                    stack.push(CFrame {
                        addr: child,
                        child_idx: 0,
                        depth: depth + 1,
                    });
                }
            } else {
                on_stack.remove(&addr);
                stack.pop();
            }
        }
    }

    (max_depth, has_recursive_call)
}

fn is_valid_addr(addr: u16, rom_end: usize) -> bool {
    addr >= ROM_BASE as u16 && (addr as usize) < rom_end && addr.is_multiple_of(2)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::disasm::disassemble;
    use crate::loader::{MEM_SIZE, ROM_BASE};

    /// Build a `Cfg` from raw bytes placed at ROM_BASE.
    fn cfg_from(bytes: &[u8]) -> Cfg {
        let mut mem = [0u8; MEM_SIZE];
        let len = bytes.len().min(MEM_SIZE - ROM_BASE);
        mem[ROM_BASE..ROM_BASE + len].copy_from_slice(&bytes[..len]);
        let instrs = disassemble(&mem, len);
        let rom_end = ROM_BASE + len;
        build(&instrs, rom_end)
    }

    fn codes(cfg: &Cfg) -> Vec<&str> {
        cfg.diags.iter().map(|d| d.code.as_str()).collect()
    }

    // -----------------------------------------------------------------------
    // Basic reachability
    // -----------------------------------------------------------------------

    #[test]
    fn single_jump_loop_is_fully_reachable() {
        // 1200 — jump 0x200 (self-loop)
        let cfg = cfg_from(&[0x12, 0x00]);
        assert!(cfg.reachable.contains_key(&0x200));
        assert!(cfg.diags.is_empty(), "{:?}", cfg.diags);
    }

    #[test]
    fn dead_code_after_unconditional_jump() {
        // 1204  (jump 0x204)
        // 6001  (set v0, 1)  ← unreachable
        // 1204  (jump 0x204) ← the actual target
        let bytes = [
            0x12, 0x04, // 0x200: jump 0x204
            0x60, 0x01, // 0x202: dead — set V0, 1
            0x12, 0x04, // 0x204: jump 0x204 (self-loop)
        ];
        let cfg = cfg_from(&bytes);
        assert!(cfg.reachable.contains_key(&0x200));
        assert!(cfg.reachable.contains_key(&0x204));
        assert!(!cfg.reachable.contains_key(&0x202), "0x202 should be dead");
    }

    #[test]
    fn call_and_ret_marks_return_site_reachable() {
        // 0x200: 2204   call 0x204
        // 0x202: 1202   jump 0x202   ← return target
        // 0x204: 00EE   ret
        let bytes = [
            0x22, 0x04, // 0x200: call 0x204
            0x12, 0x02, // 0x202: jump 0x202
            0x00, 0xEE, // 0x204: ret
        ];
        let cfg = cfg_from(&bytes);
        assert!(cfg.reachable.contains_key(&0x200));
        assert!(
            cfg.reachable.contains_key(&0x202),
            "return site must be reachable"
        );
        assert!(cfg.reachable.contains_key(&0x204));
    }

    #[test]
    fn call_discovered_after_ret_still_marks_return_site_reachable() {
        // The taken skip branch reaches RET before the not-taken branch finds
        // the CALL.  The continuation at 0x20E must nevertheless be included
        // in the conservative fixed point.
        //
        // 0x200: 3000   skip if V0 == 0
        // 0x202: 120C   jump to the late-discovered call
        // 0x204: 1208   jump to the early-discovered return
        // 0x206: 00FD   filler (unreachable)
        // 0x208: 00EE   ret
        // 0x20A: 00FD   filler (unreachable)
        // 0x20C: 2208   call 0x208
        // 0x20E: 120E   return continuation
        let bytes = [
            0x30, 0x00, 0x12, 0x0C, 0x12, 0x08, 0x00, 0xFD, 0x00, 0xEE, 0x00, 0xFD, 0x22, 0x08,
            0x12, 0x0E,
        ];
        let cfg = cfg_from(&bytes);
        assert!(
            cfg.reachable.contains_key(&0x208),
            "RET must be reachable first"
        );
        assert!(cfg.reachable.contains_key(&0x20C), "CALL must be reachable");
        assert!(
            cfg.reachable.contains_key(&0x20E),
            "late call return site must be reachable"
        );
    }

    #[test]
    fn call_depth_is_independent_of_hash_iteration_order() {
        let edges = (0..13)
            .map(|index| (0x200 + index * 4, 0x204 + index * 4))
            .collect::<Vec<_>>();
        assert_eq!(call_graph_analysis(&edges), (14, false));
    }

    #[test]
    fn skip_instruction_both_branches_reachable() {
        // 0x200: 3000  skip if V0 == 0
        // 0x202: 00E0  cls     (not-taken branch)
        // 0x204: 1204  jump 0x204  (taken branch, over 0x202)
        let bytes = [
            0x30, 0x00, // 0x200: skip if V0 == 0
            0x00, 0xE0, // 0x202: cls
            0x12, 0x04, // 0x204: jump 0x204
        ];
        let cfg = cfg_from(&bytes);
        assert!(cfg.reachable.contains_key(&0x202), "not-taken branch");
        assert!(cfg.reachable.contains_key(&0x204), "taken branch");
    }

    #[test]
    fn exit_terminates_reachability() {
        // 0x200: 00FD  exit
        // 0x202: 00E0  cls  ← unreachable
        let bytes = [0x00, 0xFD, 0x00, 0xE0];
        let cfg = cfg_from(&bytes);
        assert!(cfg.reachable.contains_key(&0x200));
        assert!(!cfg.reachable.contains_key(&0x202));
    }

    // -----------------------------------------------------------------------
    // Error diagnostics
    // -----------------------------------------------------------------------

    #[test]
    fn jump_below_rom_base_emits_e005() {
        // 1000 — jump 0x000 (into reserved area)
        let cfg = cfg_from(&[0x10, 0x00]);
        assert!(codes(&cfg).contains(&"E005"), "{:?}", cfg.diags);
    }

    #[test]
    fn jump_beyond_rom_end_emits_e006() {
        // ROM is only 2 bytes (0x200-0x201), jump to 0x400
        let cfg = cfg_from(&[0x14, 0x00]);
        assert!(codes(&cfg).contains(&"E006"), "{:?}", cfg.diags);
    }

    #[test]
    fn jump_to_odd_addr_emits_e007() {
        // 1201 — jump 0x201 (odd, misaligned)
        let bytes = [0x12, 0x01, 0x00, 0x00]; // need >= 2 bytes to be in ROM
        let cfg = cfg_from(&bytes);
        assert!(codes(&cfg).contains(&"E007"), "{:?}", cfg.diags);
    }

    #[test]
    fn indirect_jump_emits_w005() {
        // B300 — jump 0x300 + V0
        let cfg = cfg_from(&[0xB3, 0x00]);
        assert!(codes(&cfg).contains(&"W005"), "{:?}", cfg.diags);
        assert_eq!(cfg.indirect_jump_sites, vec![0x200]);
    }
}
