use chip8_verifier::{ExtensionLevel, Level, verify_bytes};

#[test]
fn rejects_an_empty_artifact_with_a_diagnostic() {
    let report = verify_bytes(&[]);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.level == Level::Error && diagnostic.code == "E001" })
    );
}

#[test]
fn accepts_xochip_roms_and_long_i_above_the_classic_ceiling() {
    // Long-I 0x1000; audio; jump 0x200. The trailing bytes make this a real
    // ROM larger than the classic 3,584-byte load window rather than a small
    // program that merely names a high address.
    let mut rom = vec![0u8; (0x1000 - 0x200) + 2];
    rom[..8].copy_from_slice(&[0xF0, 0x00, 0x10, 0x00, 0xF0, 0x02, 0x12, 0x00]);

    let report = verify_bytes(&rom);
    assert_eq!(report.extension, ExtensionLevel::XoChip);
    assert_eq!(report.validity.out_of_bounds_mem_ref_count, 0);
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.level == Level::Error),
        "{:?}",
        report.diagnostics
    );
}
