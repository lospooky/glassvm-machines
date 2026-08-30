use hexwell_core::{Emulator, MachineConfiguration, TideInput};

const ROM: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn native_tide_and_sweep_execute() {
    let mut emulator = Emulator::new(ROM, MachineConfiguration::default()).unwrap();
    emulator.set_input(TideInput::from_u64(1).unwrap());
    emulator.sweep().unwrap();
    assert_eq!(emulator.state().sweeps, 1);
}
