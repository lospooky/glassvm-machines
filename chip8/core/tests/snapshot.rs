use chip8_core::{Engine, QuirksConfig};

const ROM: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn native_snapshot_restores_machine_state() {
    let mut engine = Engine::new(ROM, QuirksConfig::default(), 11).unwrap();
    let snapshot = engine.save_snapshot();
    engine.step();
    assert_ne!(engine.cpu.pc, snapshot.pc);
    engine.load_snapshot(&snapshot);
    assert_eq!(engine.cpu.pc, snapshot.pc);
}
