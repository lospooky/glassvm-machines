use wyrd16_core::{Emulator, MachineConfiguration};

#[test]
fn equal_seed_and_artifact_produce_equal_native_continuations() {
    let rom = [0x90, 0xff, 0x70, 0x00];
    let configuration = MachineConfiguration { seed: 0xfeed_cafe };
    let mut left = Emulator::new(&rom, configuration).expect("left");
    let mut right = Emulator::new(&rom, configuration).expect("right");
    for _ in 0..32 {
        assert_eq!(left.step(), right.step());
    }
    assert_eq!(left.snapshot(), right.snapshot());
}

#[test]
fn entropy_stream_matches_the_normative_seed_vector() {
    let mut state = wyrd16_core::MachineState::boot(&[0x00, 0x00], 1).expect("boot");
    assert_eq!(
        [
            state.random_byte(),
            state.random_byte(),
            state.random_byte(),
            state.random_byte(),
        ],
        [0x00, 0x10, 0x9b, 0xf5]
    );
}
