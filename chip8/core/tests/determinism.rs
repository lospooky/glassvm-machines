use chip8_core::{EvalConfig, evaluate};

const ROM: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn repeated_native_runs_are_identical() {
    let first = evaluate(ROM, &EvalConfig::default()).unwrap();
    let second = evaluate(ROM, &EvalConfig::default()).unwrap();
    assert_eq!(first.summary.cycles, second.summary.cycles);
    assert_eq!(first.summary.frames, second.summary.frames);
    assert_eq!(first.framebuffer_flat, second.framebuffer_flat);
    assert_eq!(first.trajectory_identity, second.trajectory_identity);
}
