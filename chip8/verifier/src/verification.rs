//! Native CHIP-8 verification entry points.

use crate::analysis::{self, ExtensionLevel};
use crate::diagnostic::{Diagnostic, Level};
use crate::metrics::{BehavioralPriors, StructuralMetrics, ValidityFlags};
use crate::{cfg, disasm, loader};

/// Result of running the full verification pipeline.
pub struct VerifyReport {
    pub extension: ExtensionLevel,
    pub diagnostics: Vec<Diagnostic>,
    pub reachable_count: usize,
    pub total_count: usize,
    pub validity: ValidityFlags,
    pub structural: StructuralMetrics,
    pub behavioral: BehavioralPriors,
}

/// Static analysis report returned independently from the acceptance adapter.
pub type AnalysisReport = VerifyReport;

impl VerifyReport {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.level == Level::Error)
    }
}

/// Run the full pipeline on a file path.
pub fn verify_file(path: &std::path::Path) -> VerifyReport {
    let (rom, mut diags) = loader::load(path);
    run_pipeline(&rom.mem, rom.rom_len, &mut diags)
}

/// Run the full pipeline on raw ROM bytes (no filesystem access).
pub fn verify_bytes(data: &[u8]) -> VerifyReport {
    let mut diags: Vec<Diagnostic> = Vec::new();

    if data.is_empty() {
        diags.push(Diagnostic {
            level: Level::Error,
            code: "E001".into(),
            addr: None,
            message: "Data is empty (0 bytes)".into(),
        });
        return VerifyReport {
            extension: ExtensionLevel::Chip8,
            diagnostics: diags,
            reachable_count: 0,
            total_count: 0,
            validity: ValidityFlags::default(),
            structural: StructuralMetrics::default(),
            behavioral: BehavioralPriors::default(),
        };
    }

    if data.len() > loader::MAX_ROM_SIZE {
        diags.push(Diagnostic {
            level: Level::Error,
            code: "E002".into(),
            addr: None,
            message: format!(
                "Data is {} bytes; maximum is {}",
                data.len(),
                loader::MAX_ROM_SIZE
            ),
        });
    }

    if !data.len().is_multiple_of(2) {
        diags.push(Diagnostic {
            level: Level::Warning,
            code: "W001".into(),
            addr: None,
            message: format!(
                "Data length {} is odd — last byte cannot form a complete instruction",
                data.len()
            ),
        });
    }

    let rom_len = data.len().min(loader::MAX_ROM_SIZE);
    let mut mem = [0u8; loader::MEM_SIZE];
    mem[loader::ROM_BASE..loader::ROM_BASE + rom_len].copy_from_slice(&data[..rom_len]);

    run_pipeline(&mem, rom_len, &mut diags)
}

/// Analyze raw ROM bytes without executing them.
///
/// CHIP-8 currently derives verification and capability analysis from the
/// same tolerant structural pipeline; the public entry points remain separate
/// so their policies can evolve independently.
pub fn analyze_bytes(data: &[u8]) -> AnalysisReport {
    verify_bytes(data)
}

fn run_pipeline(
    mem: &[u8; loader::MEM_SIZE],
    rom_len: usize,
    diags: &mut Vec<Diagnostic>,
) -> VerifyReport {
    let rom_end = loader::ROM_BASE + rom_len;
    let instrs = disasm::disassemble(mem, rom_len);
    let cfg = cfg::build(&instrs, rom_end);
    diags.extend(cfg.diags.iter().cloned());
    let analysis = analysis::run(&cfg, &instrs, rom_len);
    let reachable_count = cfg.reachable.len();
    diags.extend(analysis.diags.iter().cloned());

    VerifyReport {
        extension: analysis.extension,
        diagnostics: std::mem::take(diags),
        reachable_count,
        total_count: rom_len / 2,
        validity: analysis.validity,
        structural: analysis.structural,
        behavioral: analysis.behavioral,
    }
}
