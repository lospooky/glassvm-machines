use chip8_core::{Engine, FrameResult, QuirksConfig};

const ROM: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn boots_and_steps_the_canonical_rom() {
    let mut engine = Engine::new(ROM, QuirksConfig::default(), 7).unwrap();
    assert!(matches!(engine.step_frame(), FrameResult::Ok));
    assert_eq!(engine.frame_count(), 1);
    assert!(engine.cycle_count() > 0);
}
