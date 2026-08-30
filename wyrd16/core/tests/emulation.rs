use std::collections::BTreeSet;

use wyrd16_core::{Emulator, MachineConfiguration, Rune};

fn run(rom: &[u8], cycles: usize) -> wyrd16_core::MachineState {
    let mut emulator = Emulator::new(rom, MachineConfiguration::default()).expect("emulator");
    for _ in 0..cycles {
        if emulator.state().halted {
            break;
        }
        emulator.step();
    }
    emulator.snapshot().state
}

#[test]
fn published_pixel_program_executes_in_the_native_core() {
    let rom = [0x10, 0x0a, 0x11, 0x14, 0xb0, 0x13, 0xc0, 0x10, 0x00, 0x01];
    let mut emulator = Emulator::new(&rom, MachineConfiguration::default()).expect("emulator");
    while !emulator.state().halted {
        emulator.step();
    }
    assert_eq!(emulator.state().canvas[20 * 64 + 10], 3);
    assert_eq!(emulator.state().cycles, 5);
}

#[test]
fn every_word_decodes_and_disassembles() {
    for word in 0_u16..=u16::MAX {
        let [high, low] = word.to_be_bytes();
        let rune = Rune::decode(high, low);
        assert_eq!(rune.word, word);
        assert!(!rune.disassemble().is_empty());
    }
}

#[test]
fn published_arithmetic_branch_arena_and_weave_vectors_conform() {
    let arithmetic = run(&[0x10, 0xff, 0x20, 0x02, 0x30, 0x0f, 0x00, 0x01], 8);
    assert!(arithmetic.halted);
    assert_eq!(arithmetic.registers[0], 0x0e);

    let branch = run(
        &[0x10, 0x01, 0x80, 0x01, 0x11, 0xee, 0x11, 0x2a, 0x00, 0x01],
        8,
    );
    assert_eq!(branch.registers[1], 0x2a);

    let arena = run(
        &[0x10, 0xab, 0x11, 0xff, 0x60, 0x1e, 0x52, 0x1e, 0x00, 0x01],
        8,
    );
    assert_eq!(arena.memory[0x0ffe], 0xab);
    assert_eq!(arena.registers[2], 0xab);

    let weave = run(
        &[0x10, 0x0a, 0x11, 0x14, 0xb0, 0x15, 0xf0, 0x17, 0x00, 0x01],
        8,
    );
    let lit = weave
        .canvas
        .iter()
        .enumerate()
        .filter_map(|(index, color)| (*color == 5).then_some((index % 64, index / 64)))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        lit,
        BTreeSet::from([(10, 20), (53, 20), (10, 43), (53, 43)])
    );
}
