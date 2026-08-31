//! Native TIC-80 cartridge analysis.

use sha2::{Digest, Sha256};

use crate::artifact;
use crate::report::AnalysisReport;
use crate::rules::has_compiled_callback;
use crate::structure::cartridge_structure;

pub fn analyze_bytes(bytes: &[u8]) -> Result<AnalysisReport, String> {
    let cartridge = artifact::parse(bytes)?;
    Ok(AnalysisReport {
        language: cartridge.language.clone(),
        code_bytes: cartridge.code.len(),
        chunks: cartridge_structure(&cartridge).chunks,
        has_tic_callback: has_compiled_callback(&cartridge.code, "TIC"),
        sha256: format!("{:x}", Sha256::digest(bytes)),
    })
}
