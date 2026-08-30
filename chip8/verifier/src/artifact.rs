/// Stage 0 — Binary Loader
///
/// Reads a .ch8 file into the native 64 KiB address-space buffer and performs
/// file-level structural checks before any decoding begins.
use crate::diagnostic::{Diagnostic, Level};

pub const ROM_BASE: usize = chip8_core::ROM_START as usize;
pub const MEM_SIZE: usize = chip8_core::cpu::MEM_SIZE;
pub const MAX_ROM_SIZE: usize = chip8_core::artifact::MAX_ROM_BYTES;

pub struct LoadedRom {
    /// Full XO-CHIP address space; ROM lives at [ROM_BASE..ROM_BASE+rom_len].
    pub mem: [u8; MEM_SIZE],
    /// Number of bytes read from the file
    pub rom_len: usize,
}

/// Load a .ch8 file.  Returns the loaded ROM and any file-level diagnostics.
/// Errors here are non-fatal to allow the pipeline to continue for as long as
/// possible (the CFG builder will simply see an empty/truncated ROM).
pub fn load(path: &std::path::Path) -> (LoadedRom, Vec<Diagnostic>) {
    let mut diags: Vec<Diagnostic> = Vec::new();

    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            diags.push(Diagnostic {
                level: Level::Error,
                code: "E000".into(),
                addr: None,
                message: format!("Could not read file: {e}"),
            });
            return (
                LoadedRom {
                    mem: [0u8; MEM_SIZE],
                    rom_len: 0,
                },
                diags,
            );
        }
    };

    if bytes.is_empty() {
        diags.push(Diagnostic {
            level: Level::Error,
            code: "E001".into(),
            addr: None,
            message: "File is empty (0 bytes)".into(),
        });
        return (
            LoadedRom {
                mem: [0u8; MEM_SIZE],
                rom_len: 0,
            },
            diags,
        );
    }

    if bytes.len() > MAX_ROM_SIZE {
        diags.push(Diagnostic {
            level: Level::Error,
            code: "E002".into(),
            addr: None,
            message: format!(
                "File is {} bytes; maximum is {} (0x{:X}–0x{:X})",
                bytes.len(),
                MAX_ROM_SIZE,
                ROM_BASE,
                MEM_SIZE - 1
            ),
        });
    }

    if bytes.len() % 2 != 0 {
        diags.push(Diagnostic {
            level: Level::Warning,
            code: "W001".into(),
            addr: None,
            message: format!(
                "File length {} is odd — last byte cannot form a complete instruction",
                bytes.len()
            ),
        });
    }

    let rom_len = bytes.len().min(MAX_ROM_SIZE);
    let mut mem = [0u8; MEM_SIZE];
    mem[ROM_BASE..ROM_BASE + rom_len].copy_from_slice(&bytes[..rom_len]);

    (LoadedRom { mem, rom_len }, diags)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_tmp(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(bytes).unwrap();
        f
    }

    #[test]
    fn empty_file_emits_e001() {
        let f = write_tmp(&[]);
        let (rom, diags) = load(f.path());
        assert_eq!(rom.rom_len, 0);
        assert!(diags.iter().any(|d| d.code == "E001"), "{diags:?}");
    }

    #[test]
    fn too_large_emits_e002() {
        let big = vec![0u8; MAX_ROM_SIZE + 1];
        let f = write_tmp(&big);
        let (_rom, diags) = load(f.path());
        assert!(diags.iter().any(|d| d.code == "E002"), "{diags:?}");
    }

    #[test]
    fn xo_rom_above_classic_limit_loads_without_truncation() {
        let classic_limit = 0x1000 - ROM_BASE;
        let mut bytes = vec![0u8; classic_limit + 2];
        bytes[classic_limit] = 0xAB;
        bytes[classic_limit + 1] = 0xCD;
        let f = write_tmp(&bytes);
        let (rom, diags) = load(f.path());

        assert_eq!(rom.rom_len, bytes.len());
        assert_eq!(&rom.mem[0x1000..0x1002], &[0xAB, 0xCD]);
        assert!(!diags.iter().any(|d| d.code == "E002"), "{diags:?}");
    }

    #[test]
    fn odd_length_emits_w001() {
        let f = write_tmp(&[0x00, 0xE0, 0xFF]); // 3 bytes
        let (_rom, diags) = load(f.path());
        assert!(diags.iter().any(|d| d.code == "W001"), "{diags:?}");
    }

    #[test]
    fn even_clean_rom_no_diags() {
        // 00E0 (cls) 1200 (jump 0x200) — minimal valid loop
        let f = write_tmp(&[0x00, 0xE0, 0x12, 0x00]);
        let (rom, diags) = load(f.path());
        assert_eq!(rom.rom_len, 4);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn rom_bytes_at_correct_offset() {
        let f = write_tmp(&[0xAB, 0xCD]);
        let (rom, _) = load(f.path());
        assert_eq!(rom.mem[ROM_BASE], 0xAB);
        assert_eq!(rom.mem[ROM_BASE + 1], 0xCD);
        assert_eq!(rom.mem[ROM_BASE - 1], 0x00, "font area should be zero");
    }

    #[test]
    fn missing_file_emits_e000() {
        let (_rom, diags) = load(std::path::Path::new("/nonexistent/path/test.ch8"));
        assert!(diags.iter().any(|d| d.code == "E000"), "{diags:?}");
    }
}
