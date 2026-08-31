use tic80_core::{HEIGHT, Tic80Event, Tic80Runtime, WIDTH};

const SMOKE: &[u8] = include_bytes!("../../fixtures/smoke.rom");

fn lua_cart(source: &str) -> Vec<u8> {
    let mut cart = vec![5];
    cart.extend_from_slice(&(source.len() as u16).to_le_bytes());
    cart.push(0);
    cart.extend_from_slice(source.as_bytes());
    cart
}

#[test]
fn executes_a_real_cartridge_frame_and_exposes_native_events_and_rgba() {
    let mut runtime = Tic80Runtime::new(SMOKE, 7).expect("runtime");
    let outcome = runtime.tick(0x21).expect("TIC callback");
    let framebuffer = runtime.framebuffer().expect("framebuffer");

    assert_eq!((framebuffer.width, framebuffer.height), (WIDTH, HEIGHT));
    assert_eq!(framebuffer.rgba.len(), WIDTH * HEIGHT * 4);
    assert_eq!(outcome.frame, 1);
    assert_eq!(
        outcome.events.first(),
        Some(&Tic80Event::InputSampled { mask: 0x21 })
    );
    assert_eq!(
        outcome.events.last(),
        Some(&Tic80Event::FrameCompleted { frame: 1 })
    );
    assert_eq!(runtime.snapshot().expect("state").frame, 1);
}

#[test]
fn boots_the_official_upstream_lua_demo() {
    let source = include_str!("../../fixtures/source/upstream-luademo.lua");
    let mut runtime = Tic80Runtime::new(&lua_cart(source), 7).expect("official demo runtime");
    runtime.tick(0).expect("official demo frame");
    assert_eq!(runtime.snapshot().expect("demo state").frame, 1);
}

#[test]
fn instruction_and_memory_limits_fail_closed() {
    let load_error = Tic80Runtime::new(&lua_cart("while true do end\nfunction TIC() end"), 0)
        .err()
        .expect("top-level loop must fail");
    assert!(load_error.contains("instruction budget exhausted"));

    let boot_error = Tic80Runtime::new(
        &lua_cart("function BOOT() while true do end end\nfunction TIC() end"),
        0,
    )
    .err()
    .expect("BOOT loop must fail");
    assert!(boot_error.contains("instruction budget exhausted"));

    let mut tick_loop =
        Tic80Runtime::new(&lua_cart("function TIC() while true do end end"), 0).expect("runtime");
    assert!(
        tick_loop
            .tick(0)
            .expect_err("TIC loop must fail")
            .contains("instruction budget exhausted")
    );

    let mut allocator = Tic80Runtime::new(
        &lua_cart("function TIC() local v=string.rep('x',33554432) trace(#v) end"),
        0,
    )
    .expect("runtime");
    assert!(
        allocator
            .tick(0)
            .expect_err("oversized allocation must fail")
            .to_ascii_lowercase()
            .contains("memory")
    );
}
