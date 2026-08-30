/// Stage 4 — Reporter
///
/// Formats and emits diagnostics from all stages in text or JSON.
use crate::analysis::AnalysisResult;
use crate::cfg::Cfg;
use crate::diagnostic::{Diagnostic, Level};
use crate::loader::LoadedRom;

pub enum OutputFormat {
    Text,
    Json,
}

pub struct Report<'a> {
    pub path: &'a std::path::Path,
    pub rom: &'a LoadedRom,
    pub cfg: &'a Cfg,
    pub analysis: &'a AnalysisResult,
    /// All diagnostics from every stage, in emission order.
    pub all_diags: Vec<Diagnostic>,
}

impl<'a> Report<'a> {
    pub fn emit(&self, format: &OutputFormat) {
        match format {
            OutputFormat::Text => self.emit_text(),
            OutputFormat::Json => self.emit_json(),
        }
    }

    fn emit_text(&self) {
        // Sort: errors first, then warnings, then info; within level by address.
        let mut diags = self.all_diags.clone();
        diags.sort_by(|a, b| b.level.cmp(&a.level).then(a.addr.cmp(&b.addr)));

        for d in &diags {
            let addr_part = match d.addr {
                Some(a) => format!("0x{a:03X}"),
                None => "      ".into(),
            };
            let level_str = match d.level {
                Level::Error => "error  ",
                Level::Warning => "warning",
                Level::Info => "info   ",
            };
            println!("[{}] {} {}  {}", d.code, level_str, addr_part, d.message);
        }

        // Coverage
        let reachable_count = self.cfg.reachable.len();
        let total_insns = self.rom.rom_len / 2;
        let coverage_pct = (reachable_count * 100)
            .checked_div(total_insns)
            .unwrap_or(0);

        println!();
        println!("──────────────────────────────────────────────");
        println!("  File            : {}", self.path.display());
        println!("  ROM size        : {} bytes", self.rom.rom_len);
        println!("  Classification  : {}", self.analysis.extension);
        if !self.analysis.extension_instructions.is_empty() {
            println!("  Extension insns :");
            for e in &self.analysis.extension_instructions {
                println!("    {e}");
            }
        }
        println!(
            "  Reachable insns : {} / {} ({coverage_pct}% coverage)",
            reachable_count, total_insns
        );
        let errors = diags.iter().filter(|d| d.level == Level::Error).count();
        let warnings = diags.iter().filter(|d| d.level == Level::Warning).count();
        let infos = diags.iter().filter(|d| d.level == Level::Info).count();
        println!("  Errors          : {errors}");
        println!("  Warnings        : {warnings}");
        println!("  Info            : {infos}");
        println!("──────────────────────────────────────────────");

        // Validity flags
        let v = &self.analysis.validity;
        println!();
        println!("── Validity Flags ────────────────────────────");
        println!(
            "  Valid opcode ratio       : {:.1}%",
            v.valid_opcode_ratio * 100.0
        );
        println!("  Has illegal opcode       : {}", v.has_illegal_opcode);
        println!("  CFG well-formed          : {}", v.cfg_well_formed);
        println!("  Stack underflow risk     : {}", v.stack_underflow_risk);
        println!("  Stack overflow risk      : {}", v.stack_overflow_risk);
        println!(
            "  Out-of-bounds jumps      : {}",
            v.out_of_bounds_jump_count
        );
        println!(
            "  Out-of-bounds mem refs   : {}",
            v.out_of_bounds_mem_ref_count
        );
        println!("  Side effect in loop      : {}", v.has_side_effect_in_loop);
        println!("  Has any skip             : {}", v.has_any_skip);

        // Structural metrics
        let s = &self.analysis.structural;
        println!();
        println!("── Structural Metrics ────────────────────────");
        println!(
            "  Reachable instructions   : {}",
            s.reachable_instruction_count
        );
        println!("  Basic blocks             : {}", s.basic_block_count);
        println!("  Loops (back edges)       : {}", s.loop_count);
        println!("  Max CFG depth            : {}", s.max_cfg_depth);
        println!(
            "  Reachable ratio          : {:.1}%",
            s.reachable_ratio * 100.0
        );
        println!("  Estimated code bytes     : {}", s.estimated_code_bytes);
        println!("  Estimated data bytes     : {}", s.estimated_data_bytes);

        // Behavioral priors
        let b = &self.analysis.behavioral;
        println!();
        println!("── Behavioral Priors ─────────────────────────");
        println!("  Contains draw            : {}", b.contains_draw);
        println!("  Contains key input       : {}", b.contains_key_input);
        println!("  Contains timers          : {}", b.contains_timers);
        println!("  Contains sound           : {}", b.contains_sound);
        println!(
            "  Contains collision check : {}",
            b.contains_collision_detection
        );
        println!("  Contains randomness      : {}", b.contains_randomness);
        println!("──────────────────────────────────────────────");

        if errors == 0 && warnings == 0 {
            println!("  ✓  No issues found");
        }
    }

    fn emit_json(&self) {
        use serde_json::{json, to_string_pretty};

        let reachable_count = self.cfg.reachable.len();
        let total_insns = self.rom.rom_len / 2;
        let coverage_pct = (reachable_count * 100)
            .checked_div(total_insns)
            .unwrap_or(0);

        let errors = self
            .all_diags
            .iter()
            .filter(|d| d.level == Level::Error)
            .count();
        let warnings = self
            .all_diags
            .iter()
            .filter(|d| d.level == Level::Warning)
            .count();

        let obj = json!({
            "file": self.path.to_string_lossy(),
            "rom_size_bytes": self.rom.rom_len,
            "classification": format!("{}", self.analysis.extension),
            "extension_instructions": self.analysis.extension_instructions,
            "reachable_instructions": reachable_count,
            "total_instruction_slots": total_insns,
            "coverage_percent": coverage_pct,
            "error_count": errors,
            "warning_count": warnings,
            "diagnostics": self.all_diags,
            "validity": &self.analysis.validity,
            "structural": &self.analysis.structural,
            "behavioral": &self.analysis.behavioral,
        });

        println!("{}", to_string_pretty(&obj).unwrap());
    }

    /// Exit code: 0 = clean, 1 = errors found.
    pub fn exit_code(&self) -> i32 {
        if self.all_diags.iter().any(|d| d.level == Level::Error) {
            1
        } else {
            0
        }
    }
}
