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

fn lua_cart(source: &str) -> Vec<u8> {
    let mut cart = vec![5];
    cart.extend_from_slice(&(source.len() as u16).to_le_bytes());
    cart.push(0);
    cart.extend_from_slice(source.as_bytes());
    cart
}

#[test]
fn configured_rng_seed_covers_loading_boot_frames_and_continuation() {
    let cart = lua_cart(
        "local random=math.random
        for i=0,15 do poke(0x8000+i,random(0,255)) end
        function BOOT() for i=16,31 do poke(0x8000+i,random(0,255)) end end
        function TIC() for i=32,63 do poke(0x8000+i,random(0,255)) end end",
    );
    for seed in [0, 7, i64::MAX as u64, u64::MAX] {
        let mut left = Tic80Runtime::new(&cart, seed).unwrap();
        let mut right = Tic80Runtime::new(&cart, seed).unwrap();
        assert_eq!(left.snapshot().unwrap(), right.snapshot().unwrap());
        left.tick(1).unwrap();
        right.tick(1).unwrap();
        assert_eq!(left.snapshot().unwrap(), right.snapshot().unwrap());
        let checkpoint = left.snapshot().unwrap();
        left.tick(2).unwrap();
        right
            .restore_snapshot_from_inputs(&checkpoint, [1], false)
            .unwrap();
        right.tick(2).unwrap();
        assert_eq!(left.snapshot().unwrap(), right.snapshot().unwrap());
    }
    assert_ne!(
        Tic80Runtime::new(&cart, 7).unwrap().snapshot().unwrap().ram,
        Tic80Runtime::new(&cart, 8).unwrap().snapshot().unwrap().ram
    );
}

#[test]
fn explicit_reseeding_preserves_lua_arguments_results_and_cached_aliases() {
    let cart = lua_cart(
        "local seed=math.randomseed local random=math.random
        function TIC()
          local a,b=seed(42,17) assert(a==42 and b==17)
          local x=random() assert(x>=0 and x<1)
          local x=random(1) assert(x==1)
          local x=random(-2,2) assert(x>=-2 and x<=2)
          local x=random(0) assert(math.type(x)=='integer')
          seed(42,17) pix(0,0,random(0,15))
        end",
    );
    let mut left = Tic80Runtime::new(&cart, 1).unwrap();
    let mut right = Tic80Runtime::new(&cart, 99).unwrap();
    left.tick(0).unwrap();
    right.tick(0).unwrap();
    assert_eq!(left.snapshot().unwrap(), right.snapshot().unwrap());
}

#[test]
fn host_entropy_reseeding_fails_explicitly_during_loading_boot_or_frame() {
    for call in ["math.randomseed()", "math.randomseed(nil)"] {
        for source in [
            format!("{call} function TIC() end"),
            format!("function BOOT() {call} end function TIC() end"),
        ] {
            let error = match Tic80Runtime::new(&lua_cart(&source), 7) {
                Ok(_) => panic!("host entropy was accepted"),
                Err(error) => error,
            };
            assert!(error.contains("explicit numeric seed"), "{error}");
        }
        let mut runtime =
            Tic80Runtime::new(&lua_cart(&format!("function TIC() {call} end")), 7).unwrap();
        assert!(
            runtime
                .tick(0)
                .unwrap_err()
                .contains("explicit numeric seed")
        );
    }
}
