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
fn gamepad_queries_match_held_pressed_and_repeat_semantics() {
    let source = r#"
function TIC()
    trace("btn=" .. tostring(btn(0)) .. "," .. tostring(btn(9)) .. "," .. tostring(btn()))
    trace("btnp=" .. tostring(btnp(0)) .. "," .. tostring(btnp(9)) .. "," .. tostring(btnp()))
    trace("repeat=" .. tostring(btnp(0, 2, 2)))
end
"#;
    let mut runtime = Tic80Runtime::new(&lua_cart(source), 0).expect("runtime");
    let expected = [
        (
            0x201,
            vec!["btn=true,true,513", "btnp=true,true,513", "repeat=true"],
        ),
        (
            0x201,
            vec!["btn=true,true,513", "btnp=false,false,0", "repeat=false"],
        ),
        (
            0x201,
            vec!["btn=true,true,513", "btnp=false,false,0", "repeat=true"],
        ),
        (
            0x201,
            vec!["btn=true,true,513", "btnp=false,false,0", "repeat=false"],
        ),
        (
            0,
            vec!["btn=false,false,0", "btnp=false,false,0", "repeat=false"],
        ),
        (
            0x201,
            vec!["btn=true,true,513", "btnp=true,true,513", "repeat=true"],
        ),
    ];

    for (mask, expected_traces) in expected {
        let outcome = runtime.tick(mask).expect("TIC callback");
        let traces = outcome
            .events
            .iter()
            .filter_map(|event| match event {
                Tic80Event::Trace { message } => Some(message.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(traces, expected_traces);
    }
}

#[test]
fn graphics_primitives_match_pixel_coordinates_clipping_and_default_clear() {
    let source = r#"
frame = 0
function TIC()
    frame = frame + 1
    if frame == 1 then
        cls(2)
        pix(1, 1, 3)
        line(0, 0, 3, 0, 4)
        rect(2, 0, 2, 2, 5)
        line(-2, 4, 2, 4, 8)
        rect(-1, 2, 2, 2, 6)
        rect(238, 134, 4, 4, 9)
        pix(239, 135, 7)
        trace("pix=" .. tostring(pix(1, 1)) .. "," .. tostring(pix(-1, 0))
            .. "," .. tostring(pix(240, 136)))
    else
        cls()
    end
end
"#;
    let mut runtime = Tic80Runtime::new(&lua_cart(source), 0).expect("runtime");
    let first_frame = runtime.tick(0).expect("first TIC callback");
    let trace = first_frame.events.iter().find_map(|event| match event {
        Tic80Event::Trace { message } => Some(message.as_str()),
        _ => None,
    });
    assert_eq!(trace, Some("pix=3,0,0"));

    let first = runtime.framebuffer().expect("first framebuffer");
    assert_pixel(&first, 100, 100, [0xb1, 0x3e, 0x53, 255]); // cls(2)
    assert_pixel(&first, 1, 1, [0xef, 0x7d, 0x57, 255]); // pix(1, 1, 3)
    assert_pixel(&first, 0, 0, [0xff, 0xcd, 0x75, 255]); // inclusive line endpoint
    assert_pixel(&first, 2, 0, [0xa7, 0xf0, 0x70, 255]); // rectangle overlays line
    assert_pixel(&first, 3, 1, [0xa7, 0xf0, 0x70, 255]); // rectangle width/height
    assert_pixel(&first, 4, 0, [0xb1, 0x3e, 0x53, 255]); // line endpoint is inclusive
    assert_pixel(&first, 0, 4, [0x29, 0x36, 0x6f, 255]); // line clipped at left edge
    assert_pixel(&first, 0, 2, [0x38, 0xb7, 0x64, 255]); // rectangle clipped at left edge
    assert_pixel(&first, 239, 135, [0x25, 0x71, 0x79, 255]); // bottom-right pixel
    assert_pixel(&first, 237, 135, [0xb1, 0x3e, 0x53, 255]); // rectangle clipped at screen edge

    runtime.tick(0).expect("second TIC callback");
    let cleared = runtime.framebuffer().expect("cleared framebuffer");
    assert_pixel(&cleared, 1, 1, [0x1a, 0x1c, 0x2c, 255]);
    assert_pixel(&cleared, 239, 135, [0x1a, 0x1c, 0x2c, 255]);
}

#[test]
fn palette_map_swaps_drawn_colors_and_stays_local_to_each_vram_bank() {
    let source = r#"
for offset = 0, 31 do poke(0x4020 + offset, 0) end
poke4(0x4020 * 2 + 1, 2)
poke4(0x4020 * 2 + 2, 3)
poke4(0x3ff0 * 2 + 2, 3)
poke4(0x3ff0 * 2 + 3, 2)

function TIC()
    vbank(0)
    cls(2)
    spr(1, 0, 0, 2)
    pix(10, 0, 2)
    trace("mapped=" .. peek4(0x3ff0 * 2 + 2) .. "," .. peek4(0x3ff0 * 2 + 3)
        .. "," .. pix(10, 0))
    poke4(0x3ff0 * 2 + 2, 2)
    poke4(0x3ff0 * 2 + 3, 3)
    pix(11, 0, 2)
    trace("identity=" .. pix(11, 0))

    vbank(1)
    poke(0x3ff8, 15)
    poke(0x3fc0 + 4 * 3, 0xaa)
    poke(0x3fc0 + 4 * 3 + 1, 0xbb)
    poke(0x3fc0 + 4 * 3 + 2, 0xcc)
    poke4(0x3ff0 * 2 + 2, 4)
    cls(15)
    pix(20, 20, 2)

    vbank(0)
end
"#;
    let mut runtime = Tic80Runtime::new(&lua_cart(source), 0).expect("runtime");
    let outcome = runtime.tick(0).expect("TIC callback");
    let traces = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            Tic80Event::Trace { message } => Some(message.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(traces, ["mapped=3,2,3", "identity=2"]);

    let framebuffer = runtime.framebuffer().expect("framebuffer");
    assert_pixel(&framebuffer, 100, 100, [0xef, 0x7d, 0x57, 255]); // cls(2) maps to 3
    assert_pixel(&framebuffer, 0, 0, [0x1a, 0x1c, 0x2c, 255]); // source color 0
    assert_pixel(&framebuffer, 1, 0, [0xef, 0x7d, 0x57, 255]); // source color 2 remains transparent
    assert_pixel(&framebuffer, 2, 0, [0xb1, 0x3e, 0x53, 255]); // source color 3 maps to 2
    assert_pixel(&framebuffer, 10, 0, [0xef, 0x7d, 0x57, 255]); // pix write is mapped
    assert_pixel(&framebuffer, 11, 0, [0xb1, 0x3e, 0x53, 255]); // identity map restores source color
    assert_pixel(&framebuffer, 20, 20, [0xaa, 0xbb, 0xcc, 255]); // bank 1 uses its own map
}

#[test]
fn bdr_runs_after_tic_and_selects_palette_per_display_scanline() {
    let source = r#"
function TIC()
    trace("tic")
    cls(2)
end

function BDR(row)
    trace("bdr=" .. row)
    if row == 3 then
        poke(0x3fc0 + 2 * 3, 0)
        poke(0x3fc0 + 2 * 3 + 1, 0)
        poke(0x3fc0 + 2 * 3 + 2, 255)
    elseif row == 4 then
        poke(0x3fc0 + 2 * 3, 255)
        poke(0x3fc0 + 2 * 3 + 1, 0)
        poke(0x3fc0 + 2 * 3 + 2, 0)
    elseif row == 5 then
        poke(0x3fc0 + 2 * 3, 0)
        poke(0x3fc0 + 2 * 3 + 1, 255)
        poke(0x3fc0 + 2 * 3 + 2, 0)
    end
end
"#;
    let mut runtime = Tic80Runtime::new(&lua_cart(source), 0).expect("runtime");
    let outcome = runtime.tick(0).expect("TIC and BDR callbacks");
    let traces = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            Tic80Event::Trace { message } => Some(message.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(traces.len(), 145);
    assert_eq!(traces[0], "tic");
    assert_eq!(traces[1], "bdr=0");
    assert_eq!(traces[4], "bdr=3");
    assert_eq!(traces[5], "bdr=4");
    assert_eq!(traces[6], "bdr=5");
    assert_eq!(traces[144], "bdr=143");

    let framebuffer = runtime.framebuffer().expect("framebuffer");
    assert_pixel(&framebuffer, 0, 0, [255, 0, 0, 255]); // BDR(4) renders row 0
    assert_pixel(&framebuffer, 0, 1, [0, 255, 0, 255]); // BDR(5) renders row 1
    assert_pixel(&framebuffer, 0, 2, [0, 255, 0, 255]); // latest palette remains active
    assert_pixel(&framebuffer, 0, 135, [0, 255, 0, 255]); // last display row is BDR(139)
}

#[test]
fn textured_triangles_sample_image_map_and_other_vbank_with_clip_and_colorkey() {
    let source = r#"
memset(0x4020, 0x66, 32)
poke4(0x4000 * 2, 1)
poke4(0x4000 * 2 + 1, 2)
poke4(0x4000 * 2 + 8, 3)
poke4(0x4000 * 2 + 9, 4)
mset(0, 0, 1)

function TIC()
    cls(0)
    trace("tex=" .. peek4(0x4000 * 2) .. "," .. peek4(0x4000 * 2 + 1))
    ttri(0, 0, 8, 0, 0, 8, 0, 0, 8, 0, 0, 8)
    spr(0, 70, 0, -1)
    ttri(10, 0, 18, 0, 10, 8, 0, 0, 8, 0, 0, 8, 1)

    clip(31, 0, 2, 8)
    ttri(30, 0, 38, 0, 30, 8, 0, 0, 8, 0, 0, 8)
    clip()

    ttri(40, 0, 48, 0, 40, 8, 0, 0, 8, 0, 0, 8, 0, 2)

    vbank(1)
    poke(0x3ff8, 15)
    cls(15)
    pix(20, 20, 7)
    vbank(0)
    ttri(50, 0, 58, 0, 50, 8, 20, 20, 28, 20, 20, 28, 2)
end
"#;
    let mut runtime = Tic80Runtime::new(&lua_cart(source), 0).expect("runtime");
    let outcome = runtime.tick(0).expect("TIC callback");
    let traces = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            Tic80Event::Trace { message } => Some(message.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(traces, ["tex=1,2"]);
    let framebuffer = runtime.framebuffer().expect("framebuffer");

    assert_pixel(&framebuffer, 0, 0, [0x5d, 0x27, 0x5d, 255]); // source image pixel 0
    assert_pixel(&framebuffer, 1, 0, [0xb1, 0x3e, 0x53, 255]); // source image pixel 1
    assert_pixel(&framebuffer, 0, 1, [0xef, 0x7d, 0x57, 255]); // source image next row
    assert_pixel(&framebuffer, 1, 1, [0xff, 0xcd, 0x75, 255]);
    // A texture row is eight pixels inside a tile, not a linear sheet stride.
    // Compare independent sprite and triangle paths over the same native data.
    for y in 0..2 {
        for x in 0..2 {
            let triangle = (y * WIDTH + x) * 4;
            let sprite = (y * WIDTH + 70 + x) * 4;
            assert_eq!(
                &framebuffer.rgba[triangle..triangle + 4],
                &framebuffer.rgba[sprite..sprite + 4],
            );
        }
    }
    assert_pixel(&framebuffer, 10, 0, [0x38, 0xb7, 0x64, 255]); // texsrc 1 samples map tile 1
    assert_pixel(&framebuffer, 30, 0, [0x1a, 0x1c, 0x2c, 255]); // clipped outside region
    assert_pixel(&framebuffer, 31, 0, [0xb1, 0x3e, 0x53, 255]); // clipped draw inside region
    assert_pixel(&framebuffer, 33, 0, [0x1a, 0x1c, 0x2c, 255]); // clip right edge is exclusive
    assert_pixel(&framebuffer, 40, 0, [0x5d, 0x27, 0x5d, 255]);
    assert_pixel(&framebuffer, 41, 0, [0x1a, 0x1c, 0x2c, 255]); // chromakey 2 leaves destination
    assert_pixel(&framebuffer, 50, 0, [0x25, 0x71, 0x79, 255]);
}

#[test]
fn ttri_rejects_depth_parameters_until_depth_buffer_semantics_are_implemented() {
    let source = r#"
function TIC()
    ttri(0, 0, 8, 0, 0, 8, 0, 0, 8, 0, 0, 8, 0, -1, 1, 1, 1)
end
"#;
    let mut runtime = Tic80Runtime::new(&lua_cart(source), 0).expect("runtime");
    let error = runtime
        .tick(0)
        .expect_err("depth is explicitly unsupported");
    assert!(error.contains("ttri perspective/depth coordinates are not supported yet"));
}

#[test]
fn sprites_maps_and_video_banks_match_memory_and_composition_semantics() {
    let source = r#"
for offset = 0, 31 do poke(0x4020 + offset, 0) end
poke(0x4020, 0x21)
poke(0x4021, 0x03)
mset(0, 0, 1)

function TIC()
    vbank(0)
    cls(2)
    poke(0x3fe0, 0x3c)
    spr(1, 0, 0, 0)
    map(0, 0, 1, 1, 10, 10, 0)

    local previous = vbank(1)
    poke(0x3ff8, 15)
    memset(0x3fe0, 0xab, 1)
    poke4(0x3fe0 * 2, 5)
    poke(0x3fd0, 0x6b)
    memcpy(0x3fd1, 0x3fd0, 1)
    poke(0x3fc3, 255)
    poke(0x3fc4, 0)
    poke(0x3fc5, 0)
    cls(15)
    pix(5, 5, 1)
    local overlay_transparency = peek(0x3ff8)
    local overlay_memory = peek(0x3fe0)
    local overlay_nibble = peek4(0x3fe0 * 2)
    local copied_byte = peek(0x3fd1)
    local switched_from = vbank(0)
    local base_memory = peek(0x3fe0)
    local base_transparency = peek(0x3ff8)
    trace("banks=" .. previous .. "," .. switched_from .. ","
        .. overlay_transparency .. "," .. overlay_memory .. "," .. overlay_nibble
        .. "," .. copied_byte .. "," .. base_memory .. "," .. base_transparency)
end
"#;
    let mut runtime = Tic80Runtime::new(&lua_cart(source), 0).expect("runtime");
    let outcome = runtime.tick(0).expect("TIC callback");
    let trace = outcome.events.iter().find_map(|event| match event {
        Tic80Event::Trace { message } => Some(message.as_str()),
        _ => None,
    });
    assert_eq!(trace, Some("banks=0,1,15,165,5,107,60,0"));

    let framebuffer = runtime.framebuffer().expect("framebuffer");
    assert_pixel(&framebuffer, 50, 50, [0xb1, 0x3e, 0x53, 255]); // bank-0 clear
    assert_pixel(&framebuffer, 0, 0, [0x5d, 0x27, 0x5d, 255]); // sprite pixel 0
    assert_pixel(&framebuffer, 1, 0, [0xb1, 0x3e, 0x53, 255]); // sprite pixel 1
    assert_pixel(&framebuffer, 2, 0, [0xef, 0x7d, 0x57, 255]); // sprite pixel 2
    assert_pixel(&framebuffer, 3, 0, [0xb1, 0x3e, 0x53, 255]); // transparent key
    assert_pixel(&framebuffer, 10, 10, [0x5d, 0x27, 0x5d, 255]); // mapped tile
    assert_pixel(&framebuffer, 11, 10, [0xb1, 0x3e, 0x53, 255]);
    assert_pixel(&framebuffer, 5, 5, [255, 0, 0, 255]); // bank-1 palette and overlay
    assert_pixel(&framebuffer, 6, 5, [0xb1, 0x3e, 0x53, 255]); // transparent overlay
}

#[test]
fn sprite_scale_flip_rotation_and_composite_layout_match_reference() {
    let source = r#"
for offset = 0, 63 do poke(0x4020 + offset, 0) end
poke4(0x4020 * 2 + 2 * 8 + 1, 1)
poke4(0x4020 * 2 + 1 * 8 + 5, 3)
poke4(0x4040 * 2, 4)

function TIC()
    cls(2)
    spr(1, 10, 10, 0, 2)
    spr(1, 40, 10, 0, 1, 0, 1)
    spr(1, 60, 10, 0, 1, 1, 1)
    spr(1, 80, 10, 0, 1, 1, 0)
    spr(1, 90, 10, 0, 1, 2, 0)
    spr(1, 100, 10, 0, 0)
    spr(1, 110, 10, 0, -1)
    spr(1, 130, 10, 0, 1, 0, 0, 2, 1)
end
"#;
    let mut runtime = Tic80Runtime::new(&lua_cart(source), 0).expect("runtime");
    runtime.tick(0).expect("TIC callback");
    let framebuffer = runtime.framebuffer().expect("framebuffer");

    assert_pixel(&framebuffer, 12, 14, [0x5d, 0x27, 0x5d, 255]); // scale 2
    assert_pixel(&framebuffer, 13, 15, [0x5d, 0x27, 0x5d, 255]); // scaled pixel extent
    assert_pixel(&framebuffer, 20, 12, [0xef, 0x7d, 0x57, 255]); // second source pixel
    assert_pixel(&framebuffer, 11, 10, [0xb1, 0x3e, 0x53, 255]); // color-key transparency

    assert_pixel(&framebuffer, 45, 11, [0x5d, 0x27, 0x5d, 255]); // 90-degree rotation
    assert_pixel(&framebuffer, 62, 11, [0x5d, 0x27, 0x5d, 255]); // rotate, then flip
    assert_pixel(&framebuffer, 61, 15, [0xef, 0x7d, 0x57, 255]); // combined transform
    assert_pixel(&framebuffer, 86, 12, [0x5d, 0x27, 0x5d, 255]); // horizontal flip
    assert_pixel(&framebuffer, 91, 15, [0x5d, 0x27, 0x5d, 255]); // vertical flip

    assert_pixel(&framebuffer, 101, 12, [0xb1, 0x3e, 0x53, 255]); // zero scale draws no source pixels
    assert_pixel(&framebuffer, 111, 12, [0xb1, 0x3e, 0x53, 255]); // negative scale draws no source pixels
    assert_pixel(&framebuffer, 131, 12, [0x5d, 0x27, 0x5d, 255]); // composite tile 1
    assert_pixel(&framebuffer, 138, 10, [0xff, 0xcd, 0x75, 255]); // composite tile 2
}

#[test]
fn map_remap_receives_cell_coordinates_and_applies_tile_transforms() {
    let source = r#"
for offset = 0, 31 do poke(0x4020 + offset, 0) end
poke4(0x4020 * 2 + 2 * 8 + 1, 1)
poke4(0x4040 * 2, 4)
mset(3, 4, 1)
mset(4, 4, 1)
mset(5, 4, 1)

function remap(tile, x, y)
    trace("cell=" .. x .. "," .. y .. "," .. tile)
    if x == 3 and y == 4 then return 2 end
    if x == 4 and y == 4 then return tile, 1, 1 end
    if x == 5 and y == 4 then return 0 end
    return tile
end

function TIC()
    cls(2)
    map(3, 4, 3, 1, 0, 0, 0, 1, remap)
    trace("map=" .. mget(3, 4) .. "," .. mget(4, 4) .. "," .. mget(5, 4))
end
"#;
    let mut runtime = Tic80Runtime::new(&lua_cart(source), 0).expect("runtime");
    let outcome = runtime.tick(0).expect("TIC callback");
    let traces = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            Tic80Event::Trace { message } => Some(message.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        traces,
        ["cell=3,4,1", "cell=4,4,1", "cell=5,4,1", "map=1,1,1"]
    );

    let framebuffer = runtime.framebuffer().expect("framebuffer");
    assert_pixel(&framebuffer, 0, 0, [0xff, 0xcd, 0x75, 255]); // replacement tile
    assert_pixel(&framebuffer, 10, 1, [0x5d, 0x27, 0x5d, 255]); // remapped transform
    assert_pixel(&framebuffer, 9, 2, [0xb1, 0x3e, 0x53, 255]); // source tile stayed unchanged
    assert_pixel(&framebuffer, 16, 0, [0xb1, 0x3e, 0x53, 255]); // colorkey tile is hidden
}

#[test]
fn map_wraps_lookup_and_callback_coordinates_at_native_boundaries() {
    let source = r#"
memset(0x4020, 0x11, 32)
memset(0x4040, 0x22, 32)
mset(239, 135, 1)
mset(0, 135, 2)
function TIC()
    cls()
    map(-1, -1, 2, 1, 0, 0, -1, 1, function(tile, x, y)
        trace("cell=" .. x .. "," .. y .. "," .. tile)
        return tile
    end)
end
"#;
    let mut runtime = Tic80Runtime::new(&lua_cart(source), 0).expect("runtime");
    let outcome = runtime.tick(0).expect("TIC callback");
    let traces = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            Tic80Event::Trace { message } => Some(message.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(traces, ["cell=239,135,1", "cell=0,135,2"]);
    let framebuffer = runtime.framebuffer().expect("framebuffer");
    assert_pixel(&framebuffer, 0, 0, [0x5d, 0x27, 0x5d, 255]);
    assert_pixel(&framebuffer, 8, 0, [0xb1, 0x3e, 0x53, 255]);
}

#[test]
fn clipped_clear_maps_color_once_and_negative_colorkey_keeps_color_fifteen() {
    let source = r#"
memset(0x4020, 0xff, 32)
mset(0, 0, 1)
function TIC()
    cls(0)
    poke4(0x3ff0 * 2 + 1, 2)
    poke4(0x3ff0 * 2 + 2, 3)
    clip(1, 1, 2, 2)
    cls(1)
    clip()
    spr(1, 10, 0, -1)
    map(0, 0, 1, 1, 20, 0, -1)
    clip(-2, 4, 3, 1)
    cls(1)
    clip()
end
"#;
    let mut runtime = Tic80Runtime::new(&lua_cart(source), 0).expect("runtime");
    runtime.tick(0).expect("TIC callback");
    let framebuffer = runtime.framebuffer().expect("framebuffer");
    assert_pixel(&framebuffer, 0, 0, [0x1a, 0x1c, 0x2c, 255]);
    assert_pixel(&framebuffer, 1, 1, [0xb1, 0x3e, 0x53, 255]);
    assert_pixel(&framebuffer, 2, 2, [0xb1, 0x3e, 0x53, 255]);
    assert_pixel(&framebuffer, 3, 2, [0x1a, 0x1c, 0x2c, 255]);
    assert_pixel(&framebuffer, 10, 0, [0x33, 0x3c, 0x57, 255]);
    assert_pixel(&framebuffer, 20, 0, [0x33, 0x3c, 0x57, 255]);
    assert_pixel(&framebuffer, 0, 4, [0xb1, 0x3e, 0x53, 255]);
    assert_pixel(&framebuffer, 1, 4, [0x1a, 0x1c, 0x2c, 255]);
}

fn assert_pixel(framebuffer: &tic80_core::Framebuffer, x: usize, y: usize, expected: [u8; 4]) {
    let start = (y * framebuffer.width + x) * 4;
    assert_eq!(
        &framebuffer.rgba[start..start + 4],
        expected,
        "pixel ({x}, {y})"
    );
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
