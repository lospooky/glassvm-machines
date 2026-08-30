use hexwell_core::{Emulator, MachineConfiguration};

const ROM: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn sweeps_are_deterministic() {
    let mut left = Emulator::new(ROM, MachineConfiguration { seed: 7 }).unwrap();
    let mut right = Emulator::new(ROM, MachineConfiguration { seed: 7 }).unwrap();
    assert_eq!(left.sweep().unwrap(), right.sweep().unwrap());
    assert_eq!(left.state(), right.state());
}
