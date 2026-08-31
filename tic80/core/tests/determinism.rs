use tic80_core::Tic80Runtime;

const SMOKE: &[u8] = include_bytes!("../../fixtures/smoke.rom");

#[test]
fn repeated_native_runs_are_identical() {
    let run = || {
        let mut runtime = Tic80Runtime::new(SMOKE, 7).expect("runtime");
        runtime.tick(0x11).expect("first frame");
        runtime.tick(0x20).expect("second frame");
        runtime.snapshot().expect("native snapshot")
    };

    assert_eq!(run(), run());
}
