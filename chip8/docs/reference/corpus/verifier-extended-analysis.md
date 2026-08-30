# ch8_verifier — Extended Analysis Plan

Extend the existing 5-stage static analysis pipeline with three new structured output objects
(`ValidityFlags`, `StructuralMetrics`, `BehavioralPriors`), requiring CFG enhancement with
basic-block partitioning + loop/depth analysis, three new analysis passes, and updates to the
reporter and Python bindings.

---

## New Output Schema

All new data lives in three sub-objects nested under `VerifyReport` (and mirrored in the Python
`VerifyResult`):

```
ValidityFlags
  valid_opcode_ratio          f64      valid / total ROM words (full disasm, not just reachable)
  has_illegal_opcode          bool     any OpKind::Unknown in reachable code (E004 present)
  cfg_well_formed             bool     no E003/E005/E006/E007 diagnostics
  stack_underflow_risk        bool     RET reachable from path with no call
  stack_overflow_risk         bool     max call chain depth > 12, or recursive call cycle found
  out_of_bounds_jump_count    u32      count of E003+E005+E006+E007
  out_of_bounds_mem_ref_count u32      ANNN+mem-ref pairs where computed offset exceeds ROM

StructuralMetrics
  reachable_instruction_count usize    (already in VerifyReport, formalized here)
  basic_block_count           usize    partitioned BB count in reachable CFG
  loop_count                  usize    DFS back-edge count on BB graph
  max_cfg_depth               usize    longest path (in BBs) from entry, ignoring back edges
  reachable_ratio             f64      reachable_count / total_count
  estimated_code_bytes        usize    sum of byte_len for all reachable instructions
  estimated_data_bytes        usize    rom_len − estimated_code_bytes

BehavioralPriors
  contains_draw               bool     any DRW (Dxyn) in reachable
  contains_key_input          bool     any SKP/SKNP/WaitKey in reachable
  contains_timers             bool     any SetDT/GetDT (Fx15/Fx07) in reachable
  contains_sound              bool     any SetST/SetPitch/LoadAudioBuffer in reachable
  contains_collision_detection bool    DRW followed (within BB) by instruction reading VF
  contains_randomness         bool     any RndVxByte (Cxnn) in reachable
```

---

## Implementation Phases

### Phase 1 — Data structures *(depends on nothing; do first)*

1. Create `ch8_verifier/src/metrics.rs` with `ValidityFlags`, `StructuralMetrics`,
   `BehavioralPriors`; all derive `Serialize`, `Default`, `Debug`.
2. Extend `AnalysisResult` in `analysis.rs` with `validity`, `structural`, `behavioral` fields.
3. Extend `VerifyReport` in `lib.rs` to expose all three structs.

### Phase 2 — CFG enhancement *(depends on Phase 1)*

4. Add `BasicBlock { leader: u16, addrs: Vec<u16>, successors: Vec<u16> }` to `cfg.rs`.
5. Leader identification: `0x200` + all jump/call targets + fall-through-after-skip addresses;
   computed from the successor rules already in `build()`.
6. Partition `reachable` into basic blocks after the existing worklist pass.
7. DFS on the BB graph:
   - Back-edge count → `loop_count`
   - DAG longest path → `max_cfg_depth`
8. Build call graph: collect `(caller_addr, callee_addr)` from all reachable `Call(nnn)`
   instructions; DFS with stack to find `max_call_depth` and detect recursive cycles.
9. Expose on `Cfg`: `basic_blocks: Vec<BasicBlock>`, `loop_count`, `max_cfg_depth`,
   `call_graph_edges`, `max_call_depth`, `has_recursive_call`.

### Phase 3 — New analysis passes *(depends on Phase 2)*

#### Pass E — Structural metrics *(Medium)*

10. Pull `basic_block_count`, `loop_count`, `max_cfg_depth` from enhanced `Cfg`.
11. Walk `cfg.reachable` summing `instr.byte_len` → `estimated_code_bytes`;
    `rom_len − code_bytes` → `estimated_data_bytes`.
12. `reachable_ratio = reachable_count as f64 / total_count as f64`.

#### Pass F — Behavioral priors *(Low)*

13. Single scan over `cfg.reachable.values()` using `OpKind` match:
    - `Draw(..)` → `contains_draw`
    - `SkipIfPressed | SkipIfNotPressed | WaitKey` → `contains_key_input`
    - `SetDelayTimer | GetDelayTimer` → `contains_timers`
    - `SetSoundTimer | SetPitch | LoadAudioBuffer` → `contains_sound`
    - `RndVxByte` → `contains_randomness`
14. Collision detection: within each basic block's instruction sequence, flag if any `Draw` is
    followed by an instruction where `reads_vf()` returns true (the helper already exists in
    `analysis.rs`).

#### Pass G — Validity flags *(Low / Medium)*

15. `valid_opcode_ratio`: count `OpKind::Unknown` in the **full** `Vec<Instruction>` (not just
    reachable); `(total − unknown_count) / total`.
16. `has_illegal_opcode`: `diags.iter().any(|d| d.code == "E004")`.
17. `cfg_well_formed`: no diagnostic with code in `{E003, E005, E006, E007}`.
18. `out_of_bounds_jump_count`:
    `diags.iter().filter(|d| matches!(d.code.as_str(), "E003"|"E005"|"E006"|"E007")).count()`.
19. `stack_overflow_risk`: `cfg.max_call_depth > 12 || cfg.has_recursive_call`.
20. `stack_underflow_risk`: `cfg.return_sites.is_empty() && cfg.reachable` contains any `Ret`
    (definite case); extend with reverse-path check if needed.
21. `out_of_bounds_mem_ref_count`: scan reachable instructions; for each `LD I, addr` (ANNN) at
    address *a*, find the next sequential reachable instruction at *a+2*; if it's an I-dereference
    (`FX55/FX65/DXYN/FX33/F002`), compute `I_val + access_width` in a wide integer; if it exceeds
    the XO-CHIP memory boundary `0x10000` → count++.

### Phase 4 — Reporter update *(depends on Phase 3)*

22. `emit_json()` in `reporter.rs`: add `"validity": {...}`, `"structural": {...}`,
    `"behavioral": {...}` keys — use `serde_json::to_value(&report.analysis.validity)` etc.
23. `emit_text()`: add three new headed sections after the existing summary.

### Phase 5 — Python bindings *(depends on Phase 4; parallel with Phase 4)*

24. In `ch8_verifier_py/src/lib.rs`: add `ValidityFlags`, `StructuralMetrics`, `BehavioralPriors`
    as `#[pyclass]` structs mirroring the Rust types.
25. Add to `VerifyResult`: fields for each sub-object; update `to_dict()` to include nested dicts.
26. Expose new classes in the module `#[pymodule]`.

---

## Relevant Files

| File | What changes |
|------|-------------|
| `ch8_verifier/src/metrics.rs` | **Create** — `ValidityFlags`, `StructuralMetrics`, `BehavioralPriors` structs |
| `ch8_verifier/src/cfg.rs` | `BasicBlock` type, leader set, BB partitioning, DFS for loops/depth, call graph |
| `ch8_verifier/src/analysis.rs` | Passes E, F, G; extend `AnalysisResult` |
| `ch8_verifier/src/lib.rs` | Extend `VerifyReport`; thread new fields through `run_pipeline()` |
| `ch8_verifier/src/reporter.rs` | `emit_json()` / `emit_text()` new sections |
| `ch8_verifier_py/src/lib.rs` | Mirror new structs, update `VerifyResult` |

---

## Complexity + Effort Estimates

| Feature | Complexity | Est. |
|---------|-----------|------|
| `valid_opcode_ratio` | Low | 30 min |
| `has_illegal_opcode` | Low | 15 min |
| `cfg_well_formed` | Low | 15 min |
| `out_of_bounds_jump_count` | Low | 15 min |
| `stack_underflow_risk` | Medium | 2 h |
| `stack_overflow_risk` | Medium | 2 h |
| `out_of_bounds_mem_ref_count` | Medium | 2 h |
| `basic_block_count` | Medium | 3 h |
| `loop_count` | Medium | 1.5 h |
| `max_cfg_depth` | Medium | 1 h |
| `reachable_ratio` | Trivial | 10 min |
| `estimated_code/data_bytes` | Low | 30 min |
| behavioral priors (6 flags) | Low | 1 h |
| new structs + serde | Low | 1 h |
| reporter update | Low | 1 h |
| Python bindings | Low | 1.5 h |
| **Total** | | **~18 h** |

The bulk of the work is in **Phase 2 (CFG enhancement)** — basic block partitioning + loop/depth
analysis accounts for ~5.5 h. Everything else follows from it. The four low-complexity validity
flags and all six behavioral priors are essentially single-scan passes that take < 4 h combined.

---

## Decisions / Scope Boundaries

- **Out-of-bounds mem refs**: only statically-traceable `ANNN → mem-op` pairs. Dynamic `I` (from
  `ADD I, Vx` or register-loaded I) is counted but not bounded — this is intentional; precise
  analysis requires symbolic execution.
- **Stack underflow**: the simple check (`return_sites` empty + RET reachable) covers the obvious
  case. A full dominator-tree based backward reachability check is marked optional.
- **Stack overflow threshold**: CHIP-8 hardware stack is 12 on the original COSMAC VIP; 16 on many
  modern interpreters. Plan uses 12 as the conservative threshold.
- **Loop count** = number of back edges in DFS on the BB flow graph, not the number of cycles. For
  a ROM without any CALL/indirect-jump, this is exact.
- **Collision detection** is intra-basic-block only. Inter-BB VF liveness tracking (noted in the
  existing design doc's future work) is out of scope here.
- **Python bindings**: expose new structs as both Python objects and flat `to_dict()` keys for
  maximum usability.
- **`#[serde(default)]`** on all new `VerifyReport` fields for backward compatibility with existing
  JSON consumers.

---

## Further Considerations

1. **`metrics.rs` vs. inline in `analysis.rs`**: Prefer a separate `metrics.rs` to keep the three
   public structs discoverable and to avoid making `analysis.rs` even larger. The file would be
   ≈60 lines.
2. **Test coverage**: The existing test suite in `loader.rs` can be a template; add integration
   tests with known ROMs (e.g. `bounce.ch8`) asserting that specific behavioral priors and metrics
   hit expected values.
3. **`VerifyReport` vs. reporter**: `StructuralMetrics.reachable_instruction_count` and
   `reachable_ratio` duplicate data already printed by the reporter. The plan formalizes them into
   the struct so downstream consumers (Python, JSON) get them without parsing text output.
