use crate::{
    CudaBatchEvaluator, CudaBatchOptions, CudaExecutionCounters, CudaFault, CudaRunResult, Engine,
    EvalConfig, InputEvent, KeyAction, QuirksConfig, RunPolicy, Snapshot, TerminationReason,
    effective_seed,
};
#[cfg(feature = "parallel")]
use rayon::prelude::*;
use std::sync::Mutex;
#[cfg(feature = "parallel")]
use std::time::Instant;

static CUDA_TEST_LOCK: Mutex<()> = Mutex::new(());
const SMOKE_ROM: &[u8] = include_bytes!("../../../../fixtures/smoke.rom");

struct CpuExpected {
    snapshot: Snapshot,
    requested_seed: u64,
    effective_seed: u64,
    rom_hash: u64,
    cycles: u64,
    frames: u64,
    termination: TerminationReason,
    counters: CudaExecutionCounters,
    frame_hashes: Option<Vec<u64>>,
    final_frame_hash: u64,
    fault: Option<CudaFault>,
}

struct Case {
    name: &'static str,
    rom: Vec<u8>,
    config: EvalConfig,
    fault: Option<CudaFault>,
}

fn cuda_tests_enabled() -> bool {
    std::env::var("CHIP8_CUDA_TEST").as_deref() == Ok("1")
}

fn device_ordinal() -> usize {
    std::env::var("CHIP8_CUDA_DEVICE")
        .ok()
        .map(|value| {
            value
                .parse::<usize>()
                .expect("CHIP8_CUDA_DEVICE must be a non-negative integer")
        })
        .unwrap_or(0)
}

fn evaluator(options: CudaBatchOptions) -> CudaBatchEvaluator {
    CudaBatchEvaluator::with_options(device_ordinal(), options)
        .expect("CHIP8_CUDA_TEST=1 requires a working CUDA device and embedded kernel")
}

fn cpu_expected_with_fault(
    rom: &[u8],
    config: &EvalConfig,
    expected_fault: Option<CudaFault>,
) -> CpuExpected {
    let seed = effective_seed(rom, config.seed);
    let mut engine = Engine::new(rom, config.quirks, seed).expect("conformance ROM must load");
    let cycles_per_frame = config.cycles_per_frame.max(1);
    engine.set_cycles_per_frame(cycles_per_frame);

    let summary = if let Some(script) = &config.input_script {
        let max_frames = match config.policy {
            RunPolicy::Frames(frames) | RunPolicy::UntilHalt { max_frames: frames } => frames,
            RunPolicy::Cycles(cycles) => cycles / u64::from(cycles_per_frame),
            RunPolicy::UntilStagnant { max_frames, .. } => max_frames,
        };
        engine.run_with_input_script(script, max_frames)
    } else {
        engine.run_with_policy(&config.policy)
    };
    let event_counts = crate::metrics::count_events(engine.events());
    let counters = CudaExecutionCounters {
        draw_count: summary.draw_count,
        collision_count: summary.collision_count,
        input_opcode_count: summary.input_opcode_count,
        clear_count: event_counts.clear_count,
        delay_timer_set_count: event_counts.delay_timer_set_count,
        delay_timer_nonzero_count: event_counts.delay_timer_nonzero_count,
        sound_timer_set_count: event_counts.sound_timer_set_count,
        sound_timer_nonzero_count: event_counts.sound_timer_nonzero_count,
        scroll_count: event_counts.scroll_count,
        opcode_counts: summary.coverage.opcode_counts,
    };
    let frame_hashes = config
        .record_frames
        .then(|| engine.frame_hash_history().to_vec());
    let final_frame_hash = engine.frame_hash();
    let snapshot = engine.save_snapshot();

    CpuExpected {
        snapshot,
        requested_seed: config.seed,
        effective_seed: seed,
        rom_hash: summary.rom_hash,
        cycles: summary.cycles,
        frames: summary.frames,
        termination: summary.termination,
        counters,
        frame_hashes,
        final_frame_hash,
        fault: expected_fault,
    }
}

fn cpu_expected(rom: &[u8], config: &EvalConfig) -> CpuExpected {
    cpu_expected_with_fault(rom, config, None)
}

fn assert_bytes_equal(case: &str, field: &str, actual: &[u8], expected: &[u8]) {
    assert_eq!(actual.len(), expected.len(), "{case}: {field} length");
    if let Some(index) = actual
        .iter()
        .zip(expected)
        .position(|(actual, expected)| actual != expected)
    {
        panic!(
            "{case}: {field} differs at {index:#06x}: CUDA={:#04x}, CPU={:#04x}",
            actual[index], expected[index]
        );
    }
}

fn assert_snapshot_equal(case: &str, actual: &Snapshot, expected: &Snapshot) {
    assert_bytes_equal(case, "memory", actual.mem.as_ref(), expected.mem.as_ref());
    assert_bytes_equal(
        case,
        "display plane 0",
        &actual.display_buf[0],
        &expected.display_buf[0],
    );
    assert_bytes_equal(
        case,
        "display plane 1",
        &actual.display_buf[1],
        &expected.display_buf[1],
    );
    assert_eq!(actual.v, expected.v, "{case}: registers");
    assert_eq!(actual.i, expected.i, "{case}: I");
    assert_eq!(actual.pc, expected.pc, "{case}: PC");
    assert_eq!(actual.stack, expected.stack, "{case}: stack");
    assert_eq!(actual.sp, expected.sp, "{case}: SP");
    assert_eq!(actual.dt, expected.dt, "{case}: delay timer");
    assert_eq!(actual.st, expected.st, "{case}: sound timer");
    assert_eq!(
        actual.display_hires, expected.display_hires,
        "{case}: display mode"
    );
    assert_eq!(
        actual.display_plane, expected.display_plane,
        "{case}: display plane mask"
    );
    assert_eq!(actual.keys, expected.keys, "{case}: keys");
    assert_eq!(actual.rng_state, expected.rng_state, "{case}: RNG");
    assert_eq!(actual.flags, expected.flags, "{case}: flags");
    assert_eq!(actual.audio_buf, expected.audio_buf, "{case}: audio");
    assert_eq!(
        actual.audio_pitch, expected.audio_pitch,
        "{case}: audio pitch"
    );
    assert_eq!(actual.halted, expected.halted, "{case}: halted");
    assert_eq!(actual.key_wait, expected.key_wait, "{case}: key wait");
}

fn assert_matches_cpu(case: &str, actual: &CudaRunResult, expected: &CpuExpected) {
    assert_snapshot_equal(case, &actual.snapshot, &expected.snapshot);
    assert_eq!(
        actual.requested_seed, expected.requested_seed,
        "{case}: requested seed"
    );
    assert_eq!(
        actual.effective_seed, expected.effective_seed,
        "{case}: effective seed"
    );
    assert_eq!(actual.rom_hash, expected.rom_hash, "{case}: ROM hash");
    assert_eq!(actual.cycles, expected.cycles, "{case}: cycle count");
    assert_eq!(actual.frames, expected.frames, "{case}: frame count");
    assert_eq!(
        actual.termination, expected.termination,
        "{case}: termination"
    );
    assert_eq!(actual.counters, expected.counters, "{case}: counters");
    assert_eq!(
        actual.frame_hashes, expected.frame_hashes,
        "{case}: frame hashes"
    );
    assert_eq!(
        actual.final_frame_hash, expected.final_frame_hash,
        "{case}: final display hash"
    );
    assert_eq!(actual.fault, expected.fault, "{case}: fault evidence");
    assert_eq!(actual.backend.machine_id, crate::MACHINE_ID);
    assert_eq!(actual.backend.semantics, crate::SEMANTICS);
    assert_eq!(actual.backend.kernel.symbol, "chip8_cuda_run_v1");
    assert_eq!(actual.backend.kernel.abi_version, 1);
    assert_eq!(actual.backend.kernel.source_sha256.len(), 64);
    assert_eq!(actual.backend.kernel.ptx_sha256.len(), 64);
}

#[cfg(feature = "parallel")]
fn assert_cpu_equivalent(case: &str, actual: &CpuExpected, expected: &CpuExpected) {
    assert_snapshot_equal(case, &actual.snapshot, &expected.snapshot);
    assert_eq!(
        actual.requested_seed, expected.requested_seed,
        "{case}: requested seed"
    );
    assert_eq!(
        actual.effective_seed, expected.effective_seed,
        "{case}: seed"
    );
    assert_eq!(actual.rom_hash, expected.rom_hash, "{case}: ROM hash");
    assert_eq!(actual.cycles, expected.cycles, "{case}: cycles");
    assert_eq!(actual.frames, expected.frames, "{case}: frames");
    assert_eq!(actual.termination, expected.termination, "{case}: stop");
    assert_eq!(actual.counters, expected.counters, "{case}: counters");
    assert_eq!(actual.frame_hashes, expected.frame_hashes, "{case}: hashes");
    assert_eq!(
        actual.final_frame_hash, expected.final_frame_hash,
        "{case}: final hash"
    );
    assert_eq!(actual.fault, expected.fault, "{case}: fault evidence");
}

fn assert_cuda_equivalent(case: &str, actual: &CudaRunResult, expected: &CudaRunResult) {
    assert_snapshot_equal(case, &actual.snapshot, &expected.snapshot);
    assert_eq!(
        actual.requested_seed, expected.requested_seed,
        "{case}: seed"
    );
    assert_eq!(
        actual.effective_seed, expected.effective_seed,
        "{case}: seed"
    );
    assert_eq!(actual.rom_hash, expected.rom_hash, "{case}: ROM hash");
    assert_eq!(actual.cycles, expected.cycles, "{case}: cycles");
    assert_eq!(actual.frames, expected.frames, "{case}: frames");
    assert_eq!(actual.termination, expected.termination, "{case}: stop");
    assert_eq!(actual.counters, expected.counters, "{case}: counters");
    assert_eq!(actual.frame_hashes, expected.frame_hashes, "{case}: hashes");
    assert_eq!(
        actual.final_frame_hash, expected.final_frame_hash,
        "{case}: final hash"
    );
    assert_eq!(actual.fault, expected.fault, "{case}: fault");
    assert_eq!(actual.backend.backend_id, expected.backend.backend_id);
    assert_eq!(actual.backend.kernel, expected.backend.kernel);
    assert_eq!(actual.backend.device, expected.backend.device);
}

fn run_case(evaluator: &CudaBatchEvaluator, case: &Case) {
    let outcomes = evaluator
        .evaluate_batch(&[case.rom.as_slice()], &case.config)
        .unwrap_or_else(|error| panic!("{}: CUDA backend failed: {error}", case.name));
    let actual = match outcomes.into_iter().next().expect("one lane result") {
        Ok(result) => result,
        Err(error) => panic!("{}: ROM was rejected: {error}", case.name),
    };
    let expected = cpu_expected_with_fault(&case.rom, &case.config, case.fault);
    assert_matches_cpu(case.name, &actual, &expected);
}

fn assert_cpu_projection_matches_rich_result(
    case: &str,
    rom: &[u8],
    config: &EvalConfig,
) -> CpuExpected {
    let matched = cpu_expected(rom, config);
    let rich = crate::evaluate(rom, config)
        .unwrap_or_else(|error| panic!("{case}: CPU ROM must evaluate: {error}"));
    assert_eq!(
        matched.requested_seed, config.seed,
        "{case}: requested seed"
    );
    assert_eq!(
        matched.effective_seed,
        effective_seed(rom, config.seed),
        "{case}: effective seed"
    );
    assert_eq!(matched.rom_hash, rich.summary.rom_hash, "{case}: ROM hash");
    assert_eq!(matched.cycles, rich.summary.cycles, "{case}: cycles");
    assert_eq!(matched.frames, rich.summary.frames, "{case}: frames");
    assert_eq!(
        matched.termination, rich.summary.termination,
        "{case}: termination"
    );
    assert_eq!(
        matched.counters.draw_count, rich.summary.draw_count,
        "{case}: draws"
    );
    assert_eq!(
        matched.counters.collision_count, rich.summary.collision_count,
        "{case}: collisions"
    );
    assert_eq!(
        matched.counters.input_opcode_count, rich.summary.input_opcode_count,
        "{case}: input opcodes"
    );
    assert_eq!(
        matched.counters.opcode_counts, rich.summary.coverage.opcode_counts,
        "{case}: opcode counts"
    );
    assert_eq!(
        matched.counters.clear_count, rich.interestingness.clear_count,
        "{case}: clears"
    );
    assert_eq!(
        matched.counters.delay_timer_set_count, rich.interestingness.delay_timer_set_count,
        "{case}: delay timer sets"
    );
    assert_eq!(
        matched.counters.delay_timer_nonzero_count, rich.interestingness.delay_timer_nonzero_count,
        "{case}: nonzero delay timer sets"
    );
    assert_eq!(
        matched.counters.sound_timer_set_count, rich.interestingness.sound_timer_set_count,
        "{case}: sound timer sets"
    );
    assert_eq!(
        matched.counters.sound_timer_nonzero_count, rich.interestingness.sound_timer_nonzero_count,
        "{case}: nonzero sound timer sets"
    );
    assert_eq!(
        matched.counters.scroll_count, rich.interestingness.scroll_count,
        "{case}: scrolls"
    );
    assert_eq!(
        matched.frame_hashes,
        rich.frames
            .as_ref()
            .map(|frames| { frames.iter().map(|frame| frame.hash).collect::<Vec<_>>() }),
        "{case}: frame hashes"
    );
    assert_eq!(
        rich.framebuffer_flat,
        matched.snapshot.display_buf[0]
            .iter()
            .chain(matched.snapshot.display_buf[1].iter())
            .copied()
            .collect::<Vec<_>>(),
        "{case}: final framebuffer"
    );
    assert_eq!(
        matched.final_frame_hash,
        super::hash_display(&matched.snapshot.display_buf),
        "{case}: final frame hash"
    );
    assert_eq!(matched.fault, None, "{case}: fault");
    matched
}

#[test]
fn cpu_projected_contract_matches_rich_result_common_fields() {
    let rom = vec![0x60, 0x05, 0x61, 0x05, 0xF0, 0x29, 0xD0, 0x15, 0x12, 0x06];
    let config = EvalConfig::new(RunPolicy::Frames(3))
        .with_cycles_per_frame(8)
        .with_seed(0xA5A5_5A5A_DEAD_BEEF)
        .with_frames(true);

    assert_cpu_projection_matches_rich_result("draw projection", &rom, &config);
}

fn logic_rom() -> Vec<u8> {
    vec![
        0x60, 0xF0, 0x61, 0x0F, 0x80, 0x10, 0x80, 0x11, 0x80, 0x12, 0x80, 0x13, 0x80, 0x14, 0x80,
        0x15, 0x80, 0x16, 0x80, 0x17, 0x80, 0x1E, 0x70, 0x01, 0x00, 0xFD,
    ]
}

fn skips_rom() -> Vec<u8> {
    vec![
        0x60, 0x01, 0x61, 0x01, 0x30, 0x01, 0x62, 0xAA, 0x40, 0x02, 0x62, 0xBB, 0x50, 0x10, 0x62,
        0xCC, 0x90, 0x10, 0x63, 0x03, 0x00, 0xFD,
    ]
}

fn call_rom() -> Vec<u8> {
    vec![0x22, 0x06, 0x00, 0xFD, 0x00, 0x00, 0x63, 0x07, 0x00, 0xEE]
}

fn jump_offset_rom() -> Vec<u8> {
    vec![
        0x60, 0x02, 0x62, 0x06, 0xB2, 0x08, 0x00, 0x00, 0x00, 0x00, 0x63, 0x0A, 0x12, 0x12, 0x63,
        0x0E, 0x12, 0x12, 0x00, 0xFD,
    ]
}

fn memory_rom() -> Vec<u8> {
    vec![
        0x60, 0xFE, 0x61, 0xAB, 0xA3, 0x00, 0xF1, 0x33, 0xF1, 0x55, 0x60, 0x00, 0x61, 0x00, 0xA3,
        0x00, 0xF1, 0x65, 0xAF, 0xFF, 0x60, 0x02, 0xF0, 0x1E, 0x60, 0x0F, 0xF0, 0x29, 0xF0, 0x30,
        0x00, 0xFD,
    ]
}

fn range_rom() -> Vec<u8> {
    vec![
        0x61, 0xAA, 0x62, 0xBB, 0x63, 0xCC, 0xA3, 0x00, 0x53, 0x12, 0x61, 0x00, 0x62, 0x00, 0x63,
        0x00, 0xA3, 0x00, 0x53, 0x13, 0x00, 0xFD,
    ]
}

fn self_modifying_rom() -> Vec<u8> {
    vec![
        0x60, 0x00, 0x61, 0xFD, 0xA2, 0x0C, 0xF1, 0x55, 0x12, 0x0C, 0x00, 0x00, 0x12, 0x0C,
    ]
}

fn clipping_rom() -> Vec<u8> {
    let mut rom = vec![
        0x00, 0xE0, 0x60, 0x3F, 0x61, 0x1F, 0xA2, 0x40, 0xD0, 0x11, 0x00, 0xFD,
    ];
    rom.resize(0x40, 0);
    rom.push(0xFF);
    rom
}

fn collision_rom() -> Vec<u8> {
    vec![
        0x60, 0x00, 0xF0, 0x29, 0x61, 0x00, 0x62, 0x00, 0xD1, 0x25, 0xD1, 0x25, 0x00, 0xFD,
    ]
}

fn xo_rom() -> Vec<u8> {
    let mut rom = vec![
        0x00, 0xFE, 0x00, 0xFF, 0xF3, 0x01, 0x60, 0x00, 0x61, 0x00, 0xA2, 0x80, 0xD0, 0x10, 0x00,
        0xC2, 0x00, 0xD1, 0x00, 0xFB, 0x00, 0xFC, 0x60, 0x5A, 0xF0, 0x75, 0x60, 0x00, 0xF0, 0x85,
        0xF0, 0x3A, 0xA2, 0x80, 0xF0, 0x02, 0x60, 0x09, 0xF0, 0x30, 0xF0, 0x00, 0xFF, 0xFE, 0x00,
        0xFD,
    ];
    rom.resize(0x80, 0);
    let mut sprite = [0u8; 64];
    sprite[0] = 0x80;
    sprite[1] = 0x01;
    sprite[32] = 0x40;
    rom.extend_from_slice(&sprite);
    rom
}

fn cycles_config(cycles: u64) -> EvalConfig {
    EvalConfig::new(RunPolicy::Cycles(cycles)).with_seed(0x0123_4567_89AB_CDEF)
}

fn words_rom(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_be_bytes()).collect()
}

fn words_rom_with_data(words: &[u16], data_address: u16, data: &[u8]) -> Vec<u8> {
    const ROM_BASE: usize = 0x200;
    let mut rom = words_rom(words);
    let data_offset = usize::from(data_address)
        .checked_sub(ROM_BASE)
        .expect("test data must be in ROM space");
    assert!(
        rom.len() <= data_offset,
        "test instructions overlap data at {data_address:#06x}"
    );
    rom.resize(data_offset, 0);
    rom.extend_from_slice(data);
    rom
}

fn isolated_case(name: &'static str, words: &[u16]) -> Case {
    isolated_quirk_case(name, words, QuirksConfig::default())
}

fn isolated_quirk_case(name: &'static str, words: &[u16], quirks: QuirksConfig) -> Case {
    Case {
        name,
        rom: words_rom(words),
        config: cycles_config(words.len() as u64).with_quirks(quirks),
        fault: None,
    }
}

fn isolated_data_case(
    name: &'static str,
    words: &[u16],
    data_address: u16,
    data: &[u8],
    quirks: QuirksConfig,
) -> Case {
    Case {
        name,
        rom: words_rom_with_data(words, data_address, data),
        config: cycles_config(words.len() as u64).with_quirks(quirks),
        fault: None,
    }
}

fn run_cases(evaluator: &CudaBatchEvaluator, cases: &[Case]) {
    for case in cases {
        run_case(evaluator, case);
    }
}

fn isolated_8xy_cases() -> Vec<Case> {
    let logic_reset = QuirksConfig {
        vf_reset_on_logic: true,
        ..QuirksConfig::default()
    };
    let shift_vx = QuirksConfig {
        shift_vx_only: true,
        ..QuirksConfig::default()
    };

    vec![
        // Each leaf halts immediately after the opcode under test. In particular,
        // no later arithmetic instruction can overwrite either VX or VF.
        isolated_case("8XY0 load", &[0x61A5, 0x623C, 0x8120, 0x00FD]),
        isolated_case("8XY0 load x is VF", &[0x6F11, 0x62C3, 0x8F20, 0x00FD]),
        isolated_case("8XY0 load y is VF", &[0x61A5, 0x6F3C, 0x81F0, 0x00FD]),
        isolated_case(
            "8XY1 or preserves VF",
            &[0x6150, 0x620F, 0x6FA5, 0x8121, 0x00FD],
        ),
        isolated_quirk_case(
            "8XY1 or resets VF",
            &[0x6150, 0x620F, 0x6FA5, 0x8121, 0x00FD],
            logic_reset,
        ),
        isolated_case("8XY1 or x is VF", &[0x6F50, 0x620F, 0x8F21, 0x00FD]),
        isolated_quirk_case(
            "8XY1 or x is VF and resets",
            &[0x6F50, 0x620F, 0x8F21, 0x00FD],
            logic_reset,
        ),
        isolated_case("8XY1 or y is VF", &[0x6150, 0x6F0F, 0x81F1, 0x00FD]),
        isolated_case(
            "8XY2 and preserves VF",
            &[0x61F3, 0x625A, 0x6FA5, 0x8122, 0x00FD],
        ),
        isolated_quirk_case(
            "8XY2 and resets VF",
            &[0x61F3, 0x625A, 0x6FA5, 0x8122, 0x00FD],
            logic_reset,
        ),
        isolated_case("8XY2 and x is VF", &[0x6FF3, 0x625A, 0x8F22, 0x00FD]),
        isolated_quirk_case(
            "8XY2 and x is VF and resets",
            &[0x6FF3, 0x625A, 0x8F22, 0x00FD],
            logic_reset,
        ),
        isolated_case("8XY2 and y is VF", &[0x61F3, 0x6F5A, 0x81F2, 0x00FD]),
        isolated_case(
            "8XY3 xor preserves VF",
            &[0x61F0, 0x625A, 0x6FA5, 0x8123, 0x00FD],
        ),
        isolated_quirk_case(
            "8XY3 xor resets VF",
            &[0x61F0, 0x625A, 0x6FA5, 0x8123, 0x00FD],
            logic_reset,
        ),
        isolated_case("8XY3 xor x is VF", &[0x6FF0, 0x625A, 0x8F23, 0x00FD]),
        isolated_quirk_case(
            "8XY3 xor x is VF and resets",
            &[0x6FF0, 0x625A, 0x8F23, 0x00FD],
            logic_reset,
        ),
        isolated_case("8XY3 xor y is VF", &[0x61F0, 0x6F5A, 0x81F3, 0x00FD]),
        isolated_case("8XY4 add no carry", &[0x6101, 0x6202, 0x8124, 0x00FD]),
        isolated_case("8XY4 add carry", &[0x61FE, 0x6203, 0x8124, 0x00FD]),
        isolated_case("8XY4 add x is VF", &[0x6FFE, 0x6103, 0x8F14, 0x00FD]),
        isolated_case("8XY4 add y is VF", &[0x61FE, 0x6F03, 0x81F4, 0x00FD]),
        isolated_case("8XY5 subtract equal", &[0x6102, 0x6202, 0x8125, 0x00FD]),
        isolated_case("8XY5 subtract borrow", &[0x6101, 0x6202, 0x8125, 0x00FD]),
        isolated_case("8XY5 subtract x is VF", &[0x6F01, 0x6102, 0x8F15, 0x00FD]),
        isolated_case("8XY5 subtract y is VF", &[0x6101, 0x6F02, 0x81F5, 0x00FD]),
        isolated_case(
            "8XY6 shift right from VY",
            &[0x6181, 0x6204, 0x8126, 0x00FD],
        ),
        isolated_quirk_case(
            "8XY6 shift right from VX",
            &[0x6181, 0x6204, 0x8126, 0x00FD],
            shift_vx,
        ),
        isolated_case(
            "8XY6 shift right x is VF from VY",
            &[0x6FA5, 0x6104, 0x8F16, 0x00FD],
        ),
        isolated_quirk_case(
            "8XY6 shift right x is VF from VX",
            &[0x6FA5, 0x6104, 0x8F16, 0x00FD],
            shift_vx,
        ),
        isolated_case(
            "8XY6 shift right y is VF from VY",
            &[0x6104, 0x6FA5, 0x81F6, 0x00FD],
        ),
        isolated_quirk_case(
            "8XY6 shift right y is VF from VX",
            &[0x6104, 0x6FA5, 0x81F6, 0x00FD],
            shift_vx,
        ),
        isolated_case(
            "8XY7 reverse subtract equal",
            &[0x6102, 0x6202, 0x8127, 0x00FD],
        ),
        isolated_case(
            "8XY7 reverse subtract borrow",
            &[0x6102, 0x6201, 0x8127, 0x00FD],
        ),
        isolated_case(
            "8XY7 reverse subtract x is VF",
            &[0x6F02, 0x6101, 0x8F17, 0x00FD],
        ),
        isolated_case(
            "8XY7 reverse subtract y is VF",
            &[0x6102, 0x6F01, 0x81F7, 0x00FD],
        ),
        isolated_case("8XYE shift left from VY", &[0x6181, 0x6240, 0x812E, 0x00FD]),
        isolated_quirk_case(
            "8XYE shift left from VX",
            &[0x6181, 0x6240, 0x812E, 0x00FD],
            shift_vx,
        ),
        isolated_case(
            "8XYE shift left x is VF from VY",
            &[0x6FA5, 0x6140, 0x8F1E, 0x00FD],
        ),
        isolated_quirk_case(
            "8XYE shift left x is VF from VX",
            &[0x6FA5, 0x6140, 0x8F1E, 0x00FD],
            shift_vx,
        ),
        isolated_case(
            "8XYE shift left y is VF from VY",
            &[0x6140, 0x6FA5, 0x81FE, 0x00FD],
        ),
        isolated_quirk_case(
            "8XYE shift left y is VF from VX",
            &[0x6140, 0x6FA5, 0x81FE, 0x00FD],
            shift_vx,
        ),
    ]
}

fn scroll_case(name: &'static str, opcode: u16) -> Case {
    isolated_data_case(
        name,
        &[0x00FF, 0x6008, 0x6108, 0xA220, 0xD011, opcode, 0x00FD],
        0x220,
        &[0xF0],
        QuirksConfig::xochip(),
    )
}

#[test]
fn cuda_matches_cpu_isolated_8xy_leaves_and_alias_edges() {
    if !cuda_tests_enabled() {
        eprintln!("skipping isolated CUDA 8XY parity; set CHIP8_CUDA_TEST=1 to enable it");
        return;
    }
    let _guard = CUDA_TEST_LOCK.lock().expect("CUDA test lock poisoned");
    let evaluator = evaluator(CudaBatchOptions::default());
    run_cases(&evaluator, &isolated_8xy_cases());
}

#[test]
fn cuda_matches_cpu_isolated_i_font_bcd_and_load_store_effects() {
    if !cuda_tests_enabled() {
        eprintln!("skipping isolated CUDA memory parity; set CHIP8_CUDA_TEST=1 to enable it");
        return;
    }
    let _guard = CUDA_TEST_LOCK.lock().expect("CUDA test lock poisoned");
    let evaluator = evaluator(CudaBatchOptions::default());
    let stable_i = QuirksConfig {
        load_store_no_inc_i: true,
        ..QuirksConfig::default()
    };
    let cases = vec![
        isolated_case("ANNN sets I", &[0xAABC, 0x00FD]),
        isolated_case(
            "FX1E wraps 16-bit I",
            &[0xF000, 0xFFFE, 0x6003, 0xF01E, 0x00FD],
        ),
        isolated_case("FX29 selects small font", &[0x61FE, 0xF129, 0x00FD]),
        isolated_case("FX30 selects big font", &[0x61FF, 0xF130, 0x00FD]),
        isolated_case(
            "FX33 writes isolated BCD",
            &[0x61E7, 0xA300, 0xF133, 0x00FD],
        ),
        isolated_case(
            "FX55 stores and increments I",
            &[0x6011, 0x6122, 0x6233, 0xA300, 0xF255, 0x00FD],
        ),
        isolated_quirk_case(
            "FX55 stores and preserves I",
            &[0x6011, 0x6122, 0x6233, 0xA300, 0xF255, 0x00FD],
            stable_i,
        ),
        isolated_data_case(
            "FX65 loads and increments I",
            &[0xA220, 0xF265, 0x00FD],
            0x220,
            &[0x11, 0x22, 0x33],
            QuirksConfig::default(),
        ),
        isolated_data_case(
            "FX65 loads and preserves I",
            &[0xA220, 0xF265, 0x00FD],
            0x220,
            &[0x11, 0x22, 0x33],
            stable_i,
        ),
    ];
    run_cases(&evaluator, &cases);
}

#[test]
fn cuda_matches_cpu_each_scroll_direction_in_isolation() {
    if !cuda_tests_enabled() {
        eprintln!("skipping isolated CUDA scroll parity; set CHIP8_CUDA_TEST=1 to enable it");
        return;
    }
    let _guard = CUDA_TEST_LOCK.lock().expect("CUDA test lock poisoned");
    let evaluator = evaluator(CudaBatchOptions::default());
    let cases = [
        scroll_case("00C3 scroll down", 0x00C3),
        scroll_case("00D2 scroll up", 0x00D2),
        scroll_case("00FB scroll right", 0x00FB),
        scroll_case("00FC scroll left", 0x00FC),
    ];
    run_cases(&evaluator, &cases);
}

#[test]
fn cuda_matches_cpu_stack_boundaries_and_repository_smoke() {
    if !cuda_tests_enabled() {
        eprintln!("skipping CUDA stack/smoke parity; set CHIP8_CUDA_TEST=1 to enable it");
        return;
    }
    let _guard = CUDA_TEST_LOCK.lock().expect("CUDA test lock poisoned");
    let evaluator = evaluator(CudaBatchOptions::default());
    let cases = [
        Case {
            name: "stack depth 0 underflow",
            rom: words_rom(&[0x00EE]),
            config: cycles_config(1),
            fault: None,
        },
        Case {
            name: "stack depth 1",
            rom: words_rom(&[0x2200]),
            config: cycles_config(1),
            fault: None,
        },
        Case {
            name: "stack depth 15",
            rom: words_rom(&[0x2200]),
            config: cycles_config(15),
            fault: None,
        },
        Case {
            name: "stack depth 16",
            rom: words_rom(&[0x2200]),
            config: cycles_config(16),
            fault: None,
        },
        Case {
            name: "stack depth 16 overflow attempt",
            rom: words_rom(&[0x2200]),
            config: cycles_config(17),
            fault: None,
        },
        Case {
            name: "repository smoke.rom",
            rom: SMOKE_ROM.to_vec(),
            config: EvalConfig::new(RunPolicy::Frames(4))
                .with_cycles_per_frame(4)
                .with_frames(true),
            fault: None,
        },
    ];
    run_cases(&evaluator, &cases);
}

#[test]
fn cuda_matches_cpu_opcode_quirk_extension_and_fault_matrix() {
    if !cuda_tests_enabled() {
        eprintln!("skipping CUDA parity matrix; set CHIP8_CUDA_TEST=1 to enable it");
        return;
    }
    let _guard = CUDA_TEST_LOCK.lock().expect("CUDA test lock poisoned");
    let evaluator = evaluator(CudaBatchOptions::default());
    let mut cases = Vec::new();

    for name in ["chip8", "chip48", "vip", "schip", "xochip"] {
        cases.push(Case {
            name,
            rom: logic_rom(),
            config: cycles_config(64).with_quirks(
                QuirksConfig::from_name(name).expect("documented quirk preset must resolve"),
            ),
            fault: None,
        });
    }
    cases.extend([
        Case {
            name: "conditional skips",
            rom: skips_rom(),
            config: cycles_config(64),
            fault: None,
        },
        Case {
            name: "0NNN modern no-op",
            rom: vec![0x01, 0x23, 0x00, 0xFD],
            config: cycles_config(4),
            fault: None,
        },
        Case {
            name: "call and return",
            rom: call_rom(),
            config: cycles_config(16),
            fault: None,
        },
        Case {
            name: "V0 jump offset",
            rom: jump_offset_rom(),
            config: cycles_config(16),
            fault: None,
        },
        Case {
            name: "VX jump offset",
            rom: jump_offset_rom(),
            config: cycles_config(16).with_quirks(QuirksConfig::schip()),
            fault: None,
        },
        Case {
            name: "memory and fonts incrementing I",
            rom: memory_rom(),
            config: cycles_config(64),
            fault: None,
        },
        Case {
            name: "memory and fonts stable I",
            rom: memory_rom(),
            config: cycles_config(64).with_quirks(QuirksConfig::schip()),
            fault: None,
        },
        Case {
            name: "XO reverse range load store",
            rom: range_rom(),
            config: cycles_config(64),
            fault: None,
        },
        Case {
            name: "self modifying endpoint store",
            rom: self_modifying_rom(),
            config: cycles_config(32),
            fault: None,
        },
        Case {
            name: "terminal address store wraps I",
            rom: vec![0xF0, 0x00, 0xFF, 0xFF, 0x60, 0xAA, 0xF0, 0x55, 0x00, 0xFD],
            config: cycles_config(16),
            fault: None,
        },
        Case {
            name: "clipped lores sprite",
            rom: clipping_rom(),
            config: EvalConfig::new(RunPolicy::UntilHalt { max_frames: 4 })
                .with_cycles_per_frame(4)
                .with_frames(true),
            fault: None,
        },
        Case {
            name: "VIP display-wait identity",
            rom: clipping_rom(),
            config: EvalConfig::new(RunPolicy::UntilHalt { max_frames: 4 })
                .with_cycles_per_frame(4)
                .with_quirks(QuirksConfig::vip())
                .with_frames(true),
            fault: None,
        },
        Case {
            name: "wrapped lores sprite",
            rom: clipping_rom(),
            config: EvalConfig::new(RunPolicy::UntilHalt { max_frames: 4 })
                .with_cycles_per_frame(4)
                .with_quirks(QuirksConfig {
                    clipping: false,
                    ..QuirksConfig::default()
                })
                .with_frames(true),
            fault: None,
        },
        Case {
            name: "draw collision accounting",
            rom: collision_rom(),
            config: cycles_config(16),
            fault: None,
        },
        Case {
            name: "SCHIP and XO display audio flags long index",
            rom: xo_rom(),
            config: EvalConfig::new(RunPolicy::UntilHalt { max_frames: 10 })
                .with_cycles_per_frame(8)
                .with_quirks(QuirksConfig::xochip())
                .with_frames(true),
            fault: None,
        },
        Case {
            name: "deterministic random sequence",
            rom: vec![0xC0, 0xFF, 0xC1, 0xF0, 0xC2, 0x0F, 0xC3, 0x55, 0x00, 0xFD],
            config: cycles_config(16).with_seed(0),
            fault: None,
        },
        Case {
            name: "stack underflow",
            rom: vec![0x00, 0xEE],
            config: cycles_config(1),
            fault: None,
        },
        Case {
            name: "stack overflow",
            rom: vec![0x22, 0x00],
            config: cycles_config(17),
            fault: None,
        },
        Case {
            name: "BCD memory fault",
            rom: vec![0xF0, 0x00, 0xFF, 0xFE, 0xF0, 0x33],
            config: cycles_config(4),
            fault: Some(CudaFault::Memory { address: 65_536 }),
        },
        Case {
            name: "draw memory fault",
            rom: vec![0xF0, 0x00, 0xFF, 0xE1, 0xD0, 0x10],
            config: cycles_config(4),
            fault: Some(CudaFault::Memory { address: 65_536 }),
        },
        Case {
            name: "XO range memory fault",
            rom: vec![0xF0, 0x00, 0xFF, 0xFF, 0x50, 0x12],
            config: cycles_config(4),
            fault: Some(CudaFault::Memory { address: 65_536 }),
        },
        Case {
            name: "XO audio memory fault",
            rom: vec![0xF0, 0x00, 0xFF, 0xF8, 0xF0, 0x02],
            config: cycles_config(4),
            fault: Some(CudaFault::Memory { address: 65_536 }),
        },
    ]);

    for (name, opcode) in [
        ("invalid 5 family", 0x5111u16),
        ("invalid 8 family", 0x8118),
        ("invalid 9 family", 0x9111),
        ("invalid E family", 0xE000),
        ("invalid F family", 0xF0FF),
    ] {
        cases.push(Case {
            name,
            rom: opcode.to_be_bytes().to_vec(),
            config: cycles_config(1),
            fault: Some(CudaFault::InvalidOpcode { opcode }),
        });
    }

    for case in &cases {
        run_case(&evaluator, case);
    }
}

#[test]
fn cuda_matches_cpu_frame_timing_input_rng_and_stagnation() {
    if !cuda_tests_enabled() {
        eprintln!("skipping CUDA timing parity; set CHIP8_CUDA_TEST=1 to enable it");
        return;
    }
    let _guard = CUDA_TEST_LOCK.lock().expect("CUDA test lock poisoned");
    let evaluator = evaluator(CudaBatchOptions::default());
    let cases = [
        Case {
            name: "partial terminal frame does not tick timers",
            rom: vec![0x60, 0x02, 0xF0, 0x15, 0xF0, 0x18, 0xF1, 0x07, 0x00, 0xFD],
            config: EvalConfig::new(RunPolicy::Frames(3))
                .with_cycles_per_frame(3)
                .with_frames(true),
            fault: None,
        },
        Case {
            name: "zero cycles per frame normalizes to one",
            rom: vec![0x70, 0x01, 0x12, 0x00],
            config: EvalConfig::new(RunPolicy::Frames(3))
                .with_cycles_per_frame(0)
                .with_frames(true),
            fault: None,
        },
        Case {
            name: "FX0A press hold release and key skips",
            rom: vec![
                0xF0, 0x0A, 0xE0, 0x9E, 0xE0, 0xA1, 0x60, 0xEE, 0x61, 0x01, 0x12, 0x0A,
            ],
            config: EvalConfig::new(RunPolicy::Frames(5))
                .with_cycles_per_frame(2)
                .with_frames(true)
                .with_input_script(vec![
                    InputEvent {
                        frame: 1,
                        key: 5,
                        action: KeyAction::Press,
                    },
                    InputEvent {
                        frame: 3,
                        key: 5,
                        action: KeyAction::Release,
                    },
                ]),
            fault: None,
        },
        Case {
            name: "FX0A scripted waiting termination",
            rom: vec![0xF0, 0x0A, 0x12, 0x00],
            config: EvalConfig::new(RunPolicy::Frames(2))
                .with_cycles_per_frame(1)
                .with_frames(true)
                .with_input_script(Vec::new()),
            fault: None,
        },
        Case {
            name: "FX0A plain timeout termination",
            rom: vec![0xF0, 0x0A, 0x12, 0x00],
            config: EvalConfig::new(RunPolicy::Frames(2))
                .with_cycles_per_frame(1)
                .with_frames(true),
            fault: None,
        },
        Case {
            name: "stagnation resumable frames",
            rom: vec![0x12, 0x00],
            config: EvalConfig::new(RunPolicy::UntilStagnant {
                max_frames: 10,
                window: 3,
                threshold: 0.5,
            })
            .with_cycles_per_frame(1)
            .with_frames(true),
            fault: None,
        },
        Case {
            name: "zero seed normalization",
            rom: vec![0xC0, 0xFF, 0xC1, 0xFF, 0xC2, 0xFF, 0x00, 0xFD],
            config: cycles_config(16).with_seed(effective_seed(
                &[0xC0, 0xFF, 0xC1, 0xFF, 0xC2, 0xFF, 0x00, 0xFD],
                0,
            )),
            fault: None,
        },
    ];

    for case in &cases {
        run_case(&evaluator, case);
    }

    // Exercise the resumable policy with one lane frozen by a terminal opcode
    // while its neighbor continues until the stagnation window fires.
    let config = EvalConfig::new(RunPolicy::UntilStagnant {
        max_frames: 8,
        window: 3,
        threshold: 0.5,
    })
    .with_cycles_per_frame(1)
    .with_frames(true);
    let roms: [&[u8]; 2] = [&[0x00, 0xFD], &[0x12, 0x00]];
    let actual = evaluator
        .evaluate_batch(&roms, &config)
        .expect("mixed stagnation batch must launch");
    for (index, outcome) in actual.into_iter().enumerate() {
        let result = outcome.expect("mixed stagnation ROM must load");
        let expected = cpu_expected(roms[index], &config);
        assert_matches_cpu(
            &format!("mixed stagnation lane {index}"),
            &result,
            &expected,
        );
    }
}

fn valid_results(
    options: CudaBatchOptions,
    roms: &[Vec<u8>],
    config: &EvalConfig,
) -> Vec<CudaRunResult> {
    let evaluator = evaluator(options);
    let refs: Vec<&[u8]> = roms.iter().map(Vec::as_slice).collect();
    evaluator
        .evaluate_batch(&refs, config)
        .expect("invariance batch must launch")
        .into_iter()
        .enumerate()
        .map(|(index, outcome)| {
            outcome.unwrap_or_else(|error| panic!("invariance lane {index} rejected: {error}"))
        })
        .collect()
}

#[test]
fn cuda_preserves_batch_order_chunking_and_block_invariance() {
    if !cuda_tests_enabled() {
        eprintln!("skipping CUDA invariance suite; set CHIP8_CUDA_TEST=1 to enable it");
        return;
    }
    let _guard = CUDA_TEST_LOCK.lock().expect("CUDA test lock poisoned");
    let config = EvalConfig::new(RunPolicy::Frames(3))
        .with_cycles_per_frame(8)
        .with_seed(0xA5A5_5A5A_DEAD_BEEF)
        .with_frames(true);
    let corpus = [
        vec![0x12, 0x00],
        logic_rom(),
        vec![0xC0, 0xFF, 0xC1, 0x0F, 0x12, 0x00],
        clipping_rom(),
        range_rom(),
        self_modifying_rom(),
        xo_rom(),
    ];
    let roms: Vec<Vec<u8>> = (0..256)
        .map(|index| corpus[index % corpus.len()].clone())
        .collect();

    let baseline_options = CudaBatchOptions {
        max_chunk_lanes: Some(256),
        memory_reserve_bytes: 64 * 1024 * 1024,
        threads_per_block: 256,
    };
    let chunked_options = CudaBatchOptions {
        max_chunk_lanes: Some(7),
        memory_reserve_bytes: 64 * 1024 * 1024,
        threads_per_block: 32,
    };
    let baseline = valid_results(baseline_options, &roms, &config);
    let batch_one = valid_results(baseline_options, &roms[..1], &config);
    let batch_32 = valid_results(baseline_options, &roms[..32], &config);
    let repeated = valid_results(baseline_options, &roms, &config);
    let chunked = valid_results(chunked_options, &roms, &config);
    assert_cuda_equivalent("batch size 1", &batch_one[0], &baseline[0]);
    for index in 0..batch_32.len() {
        assert_cuda_equivalent(
            &format!("batch size 32 lane {index}"),
            &batch_32[index],
            &baseline[index],
        );
    }
    for index in 0..roms.len() {
        let expected = cpu_expected(&roms[index], &config);
        assert_matches_cpu(&format!("batch lane {index}"), &baseline[index], &expected);
        assert_cuda_equivalent(
            &format!("repeat lane {index}"),
            &repeated[index],
            &baseline[index],
        );
        assert_cuda_equivalent(
            &format!("chunk/block lane {index}"),
            &chunked[index],
            &baseline[index],
        );
    }

    let reversed_roms: Vec<Vec<u8>> = roms.iter().rev().cloned().collect();
    let reversed = valid_results(chunked_options, &reversed_roms, &config);
    for (index, result) in reversed.iter().enumerate() {
        assert_cuda_equivalent(
            &format!("permuted lane {index}"),
            result,
            &baseline[roms.len() - 1 - index],
        );
    }

    // Invalid artifacts remain lane-local and cannot reorder valid neighbors.
    let evaluator = evaluator(chunked_options);
    let oversized = vec![0u8; 65_536 - 0x200 + 1];
    let valid_a = vec![0x12, 0x00];
    let valid_b = logic_rom();
    let mixed = [
        valid_a.as_slice(),
        &[][..],
        valid_b.as_slice(),
        oversized.as_slice(),
    ];
    let outcomes = evaluator
        .evaluate_batch(&mixed, &config)
        .expect("mixed valid/invalid batch must not be a backend error");
    assert_eq!(outcomes.len(), 4);
    for (index, rom) in [(0usize, valid_a.as_slice()), (2usize, valid_b.as_slice())] {
        let actual = match &outcomes[index] {
            Ok(result) => result,
            Err(error) => panic!("valid lane {index} was rejected: {error}"),
        };
        let expected = cpu_expected(rom, &config);
        assert_matches_cpu(&format!("mixed valid lane {index}"), actual, &expected);
    }
    match &outcomes[1] {
        Err(error) => assert_eq!(error, &crate::evaluate(&[], &config).unwrap_err()),
        Ok(_) => panic!("empty ROM lane unexpectedly succeeded"),
    }
    match &outcomes[3] {
        Err(error) => assert_eq!(error, &crate::evaluate(&oversized, &config).unwrap_err()),
        Ok(_) => panic!("oversized ROM lane unexpectedly succeeded"),
    }
}

fn projected_benchmark_corpus() -> [Vec<u8>; 4] {
    let draw_loop = words_rom(&[0x00E0, 0x6005, 0x6105, 0xF029, 0xD015, 0x1208]);
    let alu_rng_loop = words_rom(&[0xC0FF, 0xC10F, 0x8014, 0x1200]);
    let memory_loop = words_rom(&[
        0x6001, 0x6102, 0x6203, 0xA300, 0xF255, 0xA300, 0xF265, 0x1200,
    ]);
    [SMOKE_ROM.to_vec(), draw_loop, alu_rng_loop, memory_loop]
}

fn projected_benchmark_config() -> EvalConfig {
    EvalConfig::new(RunPolicy::Frames(60))
        .with_cycles_per_frame(12)
        .with_seed(0xA5A5_5A5A_DEAD_BEEF)
}

#[test]
fn cpu_projected_benchmark_corpus_has_fixed_contract() {
    let corpus = projected_benchmark_corpus();
    let config = projected_benchmark_config();
    for (index, rom) in corpus.iter().enumerate() {
        let result = assert_cpu_projection_matches_rich_result(
            &format!("benchmark corpus ROM {index}"),
            rom,
            &config,
        );
        assert_eq!(result.requested_seed, config.seed);
        assert_eq!(result.cycles, 720);
        assert_eq!(result.frames, 60);
        assert_eq!(result.termination, TerminationReason::Timeout);
        assert_eq!(result.frame_hashes, None);
        assert_eq!(result.fault, None);
    }
}

#[cfg(feature = "parallel")]
#[test]
fn cpu_rayon_projected_contract_preserves_order() {
    let corpus = projected_benchmark_corpus();
    let config = projected_benchmark_config();
    let order = [3usize, 0, 2, 1, 0, 3, 1, 2, 2, 0, 1, 3, 0];
    let roms: Vec<Vec<u8>> = order.iter().map(|&index| corpus[index].clone()).collect();
    let refs: Vec<&[u8]> = roms.iter().map(Vec::as_slice).collect();
    let sequential: Vec<CpuExpected> = refs.iter().map(|rom| cpu_expected(rom, &config)).collect();
    let rayon: Vec<CpuExpected> = refs
        .par_iter()
        .map(|rom| cpu_expected(rom, &config))
        .collect();

    for index in 0..roms.len() {
        assert_cpu_equivalent(
            &format!("Rayon projected lane {index}"),
            &rayon[index],
            &sequential[index],
        );
        assert_eq!(
            rayon[index].rom_hash,
            crate::emulator::fnv1a_64(&roms[index])
        );
    }
}

#[cfg(feature = "parallel")]
fn median_elapsed(samples: &[std::time::Duration]) -> std::time::Duration {
    assert!(!samples.is_empty() && samples.len() % 2 == 1);
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

#[cfg(feature = "parallel")]
fn print_benchmark_row(
    workload: &str,
    backend: &str,
    batch_size: usize,
    samples: &[std::time::Duration],
) {
    const GUEST_CYCLES_PER_LANE: f64 = 60.0 * 12.0;
    let elapsed = median_elapsed(samples);
    let seconds = elapsed.as_secs_f64();
    let lanes_per_second = batch_size as f64 / seconds;
    let guest_cycles_per_second = batch_size as f64 * GUEST_CYCLES_PER_LANE / seconds;
    let samples_ns = samples
        .iter()
        .map(|sample| sample.as_nanos().to_string())
        .collect::<Vec<_>>()
        .join(",");
    println!(
        concat!(
            "{{\"benchmark\":\"chip8_batch_projected_contract_end_to_end\",",
            "\"result_contract\":\"execution_observables_v1\",\"workload\":\"{}\",",
            "\"backend\":\"{}\",\"batch_size\":{},\"statistic\":\"median\",",
            "\"repetitions\":{},\"elapsed_ns\":{},\"samples_ns\":[{}],",
            "\"lanes_per_second\":{:.3},\"guest_cycles_per_second\":{:.3}}}"
        ),
        workload,
        backend,
        batch_size,
        samples.len(),
        elapsed.as_nanos(),
        samples_ns,
        lanes_per_second,
        guest_cycles_per_second,
    );
}

#[test]
#[ignore = "requires an explicitly leased CUDA device and is a benchmark, not a CI test"]
fn cuda_cpu_projected_contract_batch_benchmark() {
    if std::env::var("CHIP8_CUDA_BENCH").as_deref() != Ok("1") || !cuda_tests_enabled() {
        eprintln!(
            "skipping CUDA benchmark; run the ignored test with CHIP8_CUDA_TEST=1 \
             CHIP8_CUDA_BENCH=1"
        );
        return;
    }

    #[cfg(not(feature = "parallel"))]
    panic!("CUDA projected-contract benchmark requires --features cuda,parallel");

    #[cfg(feature = "parallel")]
    run_projected_contract_benchmark();
}

#[cfg(feature = "parallel")]
fn run_projected_contract_benchmark() {
    const REPETITIONS: usize = 5;
    let _guard = CUDA_TEST_LOCK.lock().expect("CUDA test lock poisoned");
    let evaluator = evaluator(CudaBatchOptions::default());
    println!("chip8_cuda_benchmark_identity={:#?}", evaluator.identity());

    let logical_cpus = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    let rayon_threads = rayon::current_num_threads();
    (0..rayon_threads.saturating_mul(4))
        .into_par_iter()
        .for_each(|index| {
            std::hint::black_box(index);
        });
    println!(
        concat!(
            "{{\"benchmark\":\"chip8_batch_projected_contract_end_to_end\",",
            "\"record\":\"environment\",\"result_contract\":",
            "\"execution_observables_v1\",\"machine_id\":\"{}\",",
            "\"semantics\":\"{}\",\"core_crate_version\":\"{}\",",
            "\"logical_cpus\":{},\"rayon_threads\":{},\"repetitions\":{}}}"
        ),
        crate::MACHINE_ID,
        crate::SEMANTICS,
        env!("CARGO_PKG_VERSION"),
        logical_cpus,
        rayon_threads,
        REPETITIONS,
    );

    // All corpus members execute exactly 720 guest cycles. The mixed workload
    // covers control flow, ALU/RNG, drawing, and memory; the same-ROM workload
    // isolates a homogeneous batch without changing the timed work contract.
    let corpus = projected_benchmark_corpus();
    let config = projected_benchmark_config();

    // One untimed launch pays first-launch effects before any samples.
    let warm_expected = cpu_expected(SMOKE_ROM, &config);
    let warm = evaluator
        .evaluate_batch(&[SMOKE_ROM], &config)
        .expect("CUDA benchmark warm-up must launch");
    let warm_result = warm[0]
        .as_ref()
        .unwrap_or_else(|error| panic!("CUDA benchmark warm-up lane failed: {error}"));
    assert_matches_cpu("benchmark warm-up", warm_result, &warm_expected);

    // CPU baselines project the same execution observables as CUDA without
    // constructing the richer public RunResult. They deliberately retain the
    // vanilla Engine's internal telemetry work; this is contract-aligned
    // implementation throughput, not internally matched work or kernel time.
    for (workload, homogeneous) in [
        ("mixed_60_frames_12_cycles", false),
        ("same_smoke_60_frames_12_cycles", true),
    ] {
        for batch_size in [1usize, 32, 256, 4096] {
            let roms: Vec<Vec<u8>> = (0..batch_size)
                .map(|index| {
                    if homogeneous {
                        corpus[0].clone()
                    } else {
                        corpus[index % corpus.len()].clone()
                    }
                })
                .collect();
            let refs: Vec<&[u8]> = roms.iter().map(Vec::as_slice).collect();
            let reference: Vec<CpuExpected> =
                refs.iter().map(|rom| cpu_expected(rom, &config)).collect();
            for result in &reference {
                assert_eq!(result.cycles, 720);
                assert_eq!(result.frames, 60);
                assert_eq!(result.termination, TerminationReason::Timeout);
                assert_eq!(result.fault, None);
            }

            let sample_cpu_sequential = || {
                let start = Instant::now();
                let output: Vec<CpuExpected> =
                    refs.iter().map(|rom| cpu_expected(rom, &config)).collect();
                std::hint::black_box(&output);
                let elapsed = start.elapsed();
                for index in 0..batch_size {
                    assert_cpu_equivalent(
                        &format!("sequential CPU lane {index}"),
                        &output[index],
                        &reference[index],
                    );
                }
                elapsed
            };
            let sample_cpu_rayon = || {
                let start = Instant::now();
                let output: Vec<CpuExpected> = refs
                    .par_iter()
                    .map(|rom| cpu_expected(rom, &config))
                    .collect();
                std::hint::black_box(&output);
                let elapsed = start.elapsed();
                for index in 0..batch_size {
                    assert_cpu_equivalent(
                        &format!("Rayon CPU lane {index}"),
                        &output[index],
                        &reference[index],
                    );
                }
                elapsed
            };
            let sample_cuda = || {
                let start = Instant::now();
                let outcomes = evaluator
                    .evaluate_batch(&refs, &config)
                    .unwrap_or_else(|error| {
                        panic!("CUDA benchmark {workload} batch {batch_size} failed: {error}")
                    });
                std::hint::black_box(&outcomes);
                let elapsed = start.elapsed();
                assert_eq!(outcomes.len(), batch_size);
                for (index, outcome) in outcomes.iter().enumerate() {
                    let result = outcome.as_ref().unwrap_or_else(|error| {
                        panic!("CUDA benchmark lane {index} failed: {error}")
                    });
                    assert_matches_cpu(
                        &format!("CUDA benchmark lane {index}"),
                        result,
                        &reference[index],
                    );
                }
                elapsed
            };

            let mut cpu_sequential_samples = Vec::with_capacity(REPETITIONS);
            let mut cpu_rayon_samples = Vec::with_capacity(REPETITIONS);
            let mut cuda_samples = Vec::with_capacity(REPETITIONS);
            for repetition in 0..REPETITIONS {
                if repetition % 2 == 0 {
                    cpu_sequential_samples.push(sample_cpu_sequential());
                    cpu_rayon_samples.push(sample_cpu_rayon());
                    cuda_samples.push(sample_cuda());
                } else {
                    cuda_samples.push(sample_cuda());
                    cpu_rayon_samples.push(sample_cpu_rayon());
                    cpu_sequential_samples.push(sample_cpu_sequential());
                }
            }

            print_benchmark_row(
                workload,
                "cpu_engine_sequential",
                batch_size,
                &cpu_sequential_samples,
            );
            print_benchmark_row(workload, "cpu_engine_rayon", batch_size, &cpu_rayon_samples);
            print_benchmark_row(workload, "cuda_driver", batch_size, &cuda_samples);
        }
    }
}
