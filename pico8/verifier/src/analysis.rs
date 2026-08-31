//! Static cartridge capability analysis.

use std::collections::BTreeSet;

use crate::artifact;
use crate::report::AnalysisReport;
use crate::structure::{artifact_format_name, cartridge_structure};

pub fn analyze_bytes(bytes: &[u8]) -> Result<AnalysisReport, String> {
    let cartridge =
        artifact::parse(bytes).map_err(|error| format!("invalid PICO-8 cartridge: {error}"))?;
    let source = &cartridge.lua;
    let callbacks = ["_init", "_update", "_update60", "_draw"]
        .into_iter()
        .filter(|name| source.contains(name))
        .map(str::to_owned)
        .collect();
    let mut api_groups = BTreeSet::new();
    for (group, names) in [
        ("display", &["pset(", "spr(", "sspr(", "map(", "cls("][..]),
        ("input", &["btn(", "btnp("][..]),
        ("audio", &["sfx(", "music("][..]),
        ("randomness", &["rnd(", "srand("][..]),
        ("memory", &["peek(", "poke(", "memcpy(", "memset("][..]),
    ] {
        if names.iter().any(|needle| source.contains(needle)) {
            api_groups.insert(group.to_owned());
        }
    }
    let structure = cartridge_structure(&cartridge);
    Ok(AnalysisReport {
        artifact_format: artifact_format_name(cartridge.format).to_owned(),
        artifact_version: cartridge.version,
        source_bytes: source.len(),
        source_lines: source.lines().count(),
        callbacks,
        api_groups,
        sections: structure.sections,
    })
}
