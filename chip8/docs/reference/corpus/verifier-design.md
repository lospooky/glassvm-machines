# ch8_verifier — Design Document

A static analysis tool for CHIP-8 ROM files (`.ch8`). Given a `.ch8` binary, it produces:

- A **classification** of what the ROM requires (CHIP-8 / SUPER-CHIP / XO-CHIP)
- A list of **errors** (definite problems — malformed encoding, out-of-bounds targets, unknown opcodes)
- A list of **warnings** (quirk-sensitive instructions that behave differently across emulators)
- A **coverage report** (what fraction of bytes were reached by CFG traversal vs. assumed data)

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Stages](#stages)
   - [Stage 0: Binary Loader](#stage-0-binary-loader)
   - [Stage 1: Disassembler](#stage-1-disassembler)
   - [Stage 2: CFG Builder](#stage-2-cfg-builder)
   - [Stage 3: Analysis Passes](#stage-3-analysis-passes)
   - [Stage 4: Reporter](#stage-4-reporter)
3. [Instruction Decoding Reference](#instruction-decoding-reference)
4. [Error Catalogue](#error-catalogue)
5. [Warning Catalogue](#warning-catalogue)
6. [Extension Classification Rules](#extension-classification-rules)
7. [Known Limitations](#known-limitations)
8. [Future Work](#future-work)

---

## Architecture Overview

```
.ch8 file
    │
    ▼
┌──────────────┐
│  Binary      │  Load bytes, basic file-level sanity checks
│  Loader      │
└──────┬───────┘
       │  raw bytes + file metadata
       ▼
┌──────────────┐
│  Disassembler│  Decode 16-bit words → Instruction structs
└──────┬───────┘
       │  instruction list (addr, opcode, operands)
       ▼
┌──────────────┐
│  CFG Builder │  Trace reachable instructions from 0x200
└──────┬───────┘
       │  control flow graph (nodes=basic blocks, edges=jumps/calls/falls)
       ▼
┌──────────────────────────────────────────┐
│  Analysis Passes (run over CFG)          │
│  ┌─────────────┐  ┌────────────────────┐ │
│  │ Validity    │  │ Quirk Sensitivity  │ │
│  │ checker     │  │ detector           │ │
│  └─────────────┘  └────────────────────┘ │
│  ┌─────────────┐  ┌────────────────────┐ │
│  │ Extension   │  │ Register Hazard    │ │
│  │ classifier  │  │ detector           │ │
│  └─────────────┘  └────────────────────┘ │
└──────────────┬───────────────────────────┘
               │  errors[], warnings[], metadata
               ▼
┌──────────────┐
│  Reporter    │  Format and emit results
└──────────────┘
```

---

## Stages

### Stage 0: Binary Loader

Responsibilities:
- Read file bytes into a buffer
- Emit **E001** if file is empty
- Emit **E002** if file is larger than 65024 bytes (`0x10000 - 0x200`)
- Emit **W001** if size is odd (last byte cannot form a complete instruction; may be padding)
- Place the ROM conceptually at `0x200` in the native 64 KiB address space;
  classic ROMs remain confined to the low 4 KiB while XO-CHIP long-I assets
  may occupy the expanded region

Output:
```
buf: [u8; 65536]  # zeroed; ROM loaded at 0x200
rom_len: usize
```

---

### Stage 1: Disassembler

Decodes every byte pair in the buffer into an `Instruction`. Does **not** filter by reachability — that comes next.

```
Instruction {
    addr:    u16,        # memory address (0x200..0xFFFF)
    word:    u16,        # raw 16-bit word
    kind:    OpKind,     # enum variant
    x:       u8,         # nibble 2 (register index, if applicable)
    y:       u8,         # nibble 3 (register index, if applicable)
    n:       u8,         # nibble 4 (4-bit literal)
    nn:      u8,         # nibbles 3-4 (8-bit literal)
    nnn:     u16,        # nibbles 2-4 (12-bit address)
}
```

`OpKind` enum has one variant per logical instruction group. Unknown patterns map to `OpKind::Unknown`.

Linear sweep: iterate every even offset from `0x200` to `0x200 + rom_len`.

---

### Stage 2: CFG Builder

Transforms the flat instruction list into a control flow graph to determine **which instructions are reachable**.

#### Algorithm: Worklist Reachability

```
reachable = {}
worklist  = { 0x200 }   # entry point

while worklist not empty:
    addr = worklist.pop()
    if addr in reachable: continue
    if addr is out of ROM bounds or odd: record E003; continue
    reachable.add(addr)
    instr = decode(addr)
    for each successor of instr:
        worklist.add(successor)
```

#### Successor rules

| Instruction type | Successors |
|-----------------|-----------|
| Normal (fall-through) | `addr + 2` |
| Skip (`3XNN`, `4XNN`, `5XY0`, `9XY0`, `EX9E`, `EXA1`) | `addr + 2` (not taken), `addr + 4` (taken) |
| `1NNN` (jump) | `NNN` only |
| `2NNN` (call) | `NNN` (callee entry); `addr + 2` added when `00EE` returns |
| `00EE` (return) | none — pops from logical call stack |
| `BNNN` (jump0) | **unresolvable** — mark address as indirect-jump site; no edge added |
| `00FD` (exit) | none |
| `FX0A` (wait key) | `addr + 2` |

For returns, maintain a **call stack set**: every `2NNN` call records `addr + 2` as a legal return destination; `00EE` adds all such return sites to the worklist.

#### Indirect jumps (`BNNN` / `jump0`)

These cannot be statically resolved without value analysis. Log a **W005** warning and leave the target region unmarked. In a future pass, simple patterns (`jump0 TABLE` preceded by `v0 := constant`) can be resolved.

---

### Stage 3: Analysis Passes

All passes operate only on **reachable** instructions from the CFG. Unreachable bytes are classified as probable data.

---

#### Pass A: Validity Checker

| Check | Error |
|-------|-------|
| `OpKind::Unknown` in reachable code | E004 |
| Jump/call target (`NNN`) < `0x200` | E005 (jumps into reserved/font area) |
| Jump/call target > `0x200 + rom_len` | E006 (out of ROM bounds) |
| Jump/call target is an odd address | E007 |
| `0NNN` raw machine call | E008 |
| `DXYN` with N == 0 in lo-res context | not an error, triggers SCHIP classification |

---

#### Pass B: Extension Classifier

Walk reachable instructions and collect the maximum extension level required.

Levels (in order): `CHIP8 < SUPERCHIP < XO_CHIP`

| Instruction(s) seen | Requires |
|--------------------|---------|
| `00FF`, `00FE` | SUPER-CHIP |
| `DXY0` (N=0 sprite) | SUPER-CHIP |
| `FX30` | SUPER-CHIP |
| `FX75`, `FX85` with X > 7 | XO-CHIP |
| `00DN` (scroll-up) | XO-CHIP |
| `5XY2`, `5XY3` | XO-CHIP |
| `F000` (long address load) | XO-CHIP |
| `FN01` (plane select) | XO-CHIP |
| `F002` (audio) | XO-CHIP |
| `FX3A` (pitch) | XO-CHIP |

---

#### Pass C: Quirk Sensitivity Detector

These instructions behave differently between original CHIP-8 and SUPER-CHIP. Emit warnings for each.

| Condition | Warning |
|-----------|---------|
| `8XY6` or `8XYE` where `X != Y` | W002 — shift quirk: VX=VY>>1 on VIP, VX=VX>>1 on SCHIP |
| `FX55` or `FX65` and the next reachable use of `i` does not reinitialize it | W003 — store/load quirk: I increments on VIP, unchanged on SCHIP |
| `BNNN` | W004 — jump-offset quirk: V0+NNN on VIP, VX+XNN on SCHIP |
| `8XY1`, `8XY2`, `8XY3` and the instruction immediately after reads `vf` | W006 — VF clobber quirk: OR/AND/XOR reset VF on VIP, preserve on SCHIP |

---

#### Pass D: Register Hazard Detector

Track register liveness (backward analysis) to find suspicious patterns:

| Pattern | Warning |
|---------|---------|
| `VF` read as a general-purpose input immediately after an instruction that also writes `VF` as a flag | W007 — VF used as both flag and data input |
| `VF` written by a non-flag instruction (`6F NN`, `8XY0` targeting F) | W008 — VF explicitly written; overwriting flag behavior |

**Scope:** only apply within a single basic block for the initial version. Interprocedural analysis is future work.

---

### Stage 4: Reporter

Formats collected diagnostics and metadata.

Output modes:
- **`text`** (default) — human-readable, one line per issue with address and description
- **`json`** — machine-readable, structured list of diagnostics

Each diagnostic:
```json
{
  "level": "error" | "warning" | "info",
  "code": "E004",
  "addr": "0x022A",
  "message": "Unknown opcode 0xE3AC at 0x022A"
}
```

Summary block:
```
Classification: SUPER-CHIP
Reachable instructions: 312 (coverage: 78%)
Errors:   0
Warnings: 3
```

Exit codes:
- `0` — no errors
- `1` — errors found
- `2` — tool failure (bad arguments, unreadable file)

---

## Instruction Decoding Reference

All CHIP-8 instructions are 16-bit, big-endian. Decode with:

```
word   = (buf[addr] << 8) | buf[addr+1]
type   = (word >> 12) & 0xF   # top nibble
x      = (word >>  8) & 0xF
y      = (word >>  4) & 0xF
n      =  word        & 0xF
nn     =  word        & 0xFF
nnn    =  word        & 0xFFF
```

| Top nibble | Instruction(s) |
|:----------:|---------------|
| `0` | `00E0` clear, `00EE` return, `00CN` scroll-down, `00FB` scroll-right, `00FC` scroll-left, `00FD` exit, `00FE` lores, `00FF` hires, `00DN` scroll-up (XO), `0NNN` raw call (legacy) |
| `1` | `1NNN` jump |
| `2` | `2NNN` call |
| `3` | `3XNN` skip if VX==NN |
| `4` | `4XNN` skip if VX!=NN |
| `5` | `5XY0` skip if VX==VY, `5XY2` range-save (XO), `5XY3` range-load (XO) |
| `6` | `6XNN` set VX = NN |
| `7` | `7XNN` add NN to VX (no carry) |
| `8` | `8XY0`–`8XYE` ALU ops |
| `9` | `9XY0` skip if VX!=VY |
| `A` | `ANNN` set I = NNN |
| `B` | `BNNN` jump with offset |
| `C` | `CXNN` random |
| `D` | `DXYN` draw sprite |
| `E` | `EX9E` skip if key, `EXA1` skip if not key |
| `F` | timer/memory/misc (see table in chip8.md), `F000` long addr (XO), `FN01` plane (XO), `F002` audio (XO), `FX3A` pitch (XO) |

---

## Error Catalogue

| Code | Level | Description |
|------|-------|-------------|
| E001 | error | File is empty (0 bytes) |
| E002 | error | File exceeds 65024 bytes (too large for 64 KiB address space from 0x200) |
| E003 | error | CFG edge targets an out-of-bounds or odd address |
| E004 | error | Unknown/invalid opcode in reachable code |
| E005 | error | Jump/call into reserved area (< 0x200) |
| E006 | error | Jump/call target beyond ROM end |
| E007 | error | Jump/call target is an odd address (misaligned) |
| E008 | error | `0NNN` raw machine call — not valid outside original COSMAC VIP |

---

## Warning Catalogue

| Code | Level | Description |
|------|-------|-------------|
| W001 | warning | File length is odd — last byte cannot form a complete instruction |
| W002 | warning | Shift instruction `8XY6`/`8XYE` where X≠Y — behavior differs (VIP vs SCHIP) |
| W003 | warning | `FX55`/`FX65` may rely on I post-increment — broken on SUPER-CHIP |
| W004 | warning | `BNNN` jump-with-offset — offset register is V0 on VIP, VX on SCHIP |
| W005 | info | `jump0` indirect jump — target region not statically resolvable |
| W006 | warning | `8XY1/2/3` OR/AND/XOR followed by VF read — VF clobber behavior differs (VIP vs SCHIP) |
| W007 | warning | `VF` used as both arithmetic flag output and program data input |
| W008 | info | `VF` directly written by program — intentional overriding of flag register |

---

## Extension Classification Rules

The highest requirement found in reachable code determines the classification:

```
if any XO-CHIP instruction seen:
    classification = "XO-CHIP"
elif any SUPER-CHIP instruction seen:
    classification = "SUPER-CHIP"
else:
    classification = "CHIP-8"
```

The report also lists which specific extension instructions were found, so users know exactly why they can't run a ROM on a plain CHIP-8 emulator.

---

## Known Limitations

1. **Code/data ambiguity.** The linear sweep in Stage 1 decodes all even byte pairs, including bytes that are actually sprite/font data. The CFG builder mitigates this by only analyzing *reachable* instructions, but self-referential ROMs that jump into the middle of a data table will still produce false positives.

2. **Indirect jumps (`jump0`).** Without value range tracking, `BNNN` and `jump0` destinations are unresolvable. A future pass could handle the common pattern of an immediately-preceding `v0 := const` or a bounded `v0 &= n`.

3. **Self-modifying code.** Programs that write instruction bytes at runtime (e.g. via the trampoline / `:unpack` pattern in Octo) cannot be statically verified in those regions. Such regions will likely show up as unreachable or produce spurious decode errors.

4. **VF hazard analysis is intra-block only.** The register hazard detector doesn't track VF state across basic block boundaries in v1.

5. **FX55/FX65 I-tracking is approximate.** Detecting whether I is "reinitialized before next use" requires data-flow analysis. The initial pass uses a simple heuristic: if the instruction immediately following `FX55`/`FX65` is not `ANNN` or `i += vx`, emit the warning.

---

## Future Work

- **V0 value range tracking** — enables resolving `jump0` destinations
- **Interprocedural VF liveness** — track VF hazards across subroutine call/return edges
- **Symbolic execution mode** — for deeper reachability analysis
- **Disassembly output mode** — emit annotated assembly alongside diagnostics
- **Batch mode** — verify an entire directory of ROMs, produce summary report
- **MEGA-CHIP8 classification** — add opcodes `0010`, `0011`, `00BN`, etc.
