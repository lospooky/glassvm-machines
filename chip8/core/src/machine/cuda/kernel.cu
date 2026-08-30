// Native CUDA CHIP-8/SUPER-CHIP/XO-CHIP batch interpreter.
//
// This translation unit intentionally includes no CUDA or C runtime headers.
// It is suitable for both nvcc and NVRTC.  The public structs below are the
// complete v1 C ABI and are mirrored by Rust `#[repr(C)]` definitions.
//
// Large mutable buffers use an address-major (transposed) layout so a warp of
// machines executing the same operation accesses adjacent bytes:
//
//   memory_soa[address * lane_count + lane]
//   display_soa[(plane * 8192 + physical_pixel) * lane_count + lane]
//
// Input masks and frame hashes are lane-major because they are ordinarily
// staged/extracted a complete lane at a time:
//
//   input_masks[lane * input_stride + frame]
//   frame_hashes[lane * frame_hash_stride + completed_frame]

typedef unsigned char chip8_u8;
typedef unsigned short chip8_u16;
typedef unsigned int chip8_u32;
typedef unsigned long long chip8_u64;

static_assert(sizeof(chip8_u8) == 1, "CHIP-8 CUDA ABI requires 8-bit bytes");
static_assert(sizeof(chip8_u16) == 2, "CHIP-8 CUDA ABI requires 16-bit shorts");
static_assert(sizeof(chip8_u32) == 4, "CHIP-8 CUDA ABI requires 32-bit ints");
static_assert(sizeof(chip8_u64) == 8, "CHIP-8 CUDA ABI requires 64-bit long longs");

enum Chip8CudaAbiConstants {
    CHIP8_CUDA_ABI_VERSION = 1,
    CHIP8_MEMORY_SIZE = 65536,
    CHIP8_DISPLAY_WIDTH = 128,
    CHIP8_DISPLAY_HEIGHT = 64,
    CHIP8_DISPLAY_PLANE_SIZE = 8192,
};

enum Chip8CudaQuirkFlags {
    CHIP8_QUIRK_SHIFT_VX_ONLY = 1u << 0,
    CHIP8_QUIRK_LOAD_STORE_NO_INC_I = 1u << 1,
    // Accepted for semantic identity with the CPU configuration.  The CPU
    // family currently records this quirk but does not delay DXYN, so the CUDA
    // interpreter likewise leaves it without an execution-time effect.
    CHIP8_QUIRK_DISPLAY_WAIT = 1u << 2,
    CHIP8_QUIRK_CLIPPING = 1u << 3,
    CHIP8_QUIRK_VF_RESET_ON_LOGIC = 1u << 4,
    CHIP8_QUIRK_JUMP_OFFSET_VX = 1u << 5,
    CHIP8_QUIRK_MASK = (1u << 6) - 1u,
};

enum Chip8CudaKeyWaitKind {
    CHIP8_KEY_WAIT_NONE = 0,
    CHIP8_KEY_WAIT_PRESS = 1,
    CHIP8_KEY_WAIT_RELEASE = 2,
};

enum Chip8CudaStatus {
    CHIP8_STATUS_TIMEOUT = 0,
    CHIP8_STATUS_HALTED = 1,
    CHIP8_STATUS_WAITING_FOR_INPUT = 2,
    CHIP8_STATUS_INVALID_OPCODE = 3,
    CHIP8_STATUS_STACK_OVERFLOW = 4,
    CHIP8_STATUS_STACK_UNDERFLOW = 5,
    CHIP8_STATUS_MEMORY_FAULT = 6,
    CHIP8_STATUS_INVALID_CONFIGURATION = 7,
};

enum Chip8CudaRunMode {
    // Frame mode mirrors Engine::step_frame and the dense input-mask form of
    // run_with_input_script.
    CHIP8_RUN_FRAMES = 0,
    // Cycle mode mirrors Engine::run_cycles: no input application, timer tick,
    // completed frame, or frame-history write occurs.
    CHIP8_RUN_CYCLES = 1,
};

// All strides and capacities are element counts rather than byte counts.
struct Chip8CudaConfig {
    chip8_u32 abi_version;
    chip8_u32 lane_count;
    chip8_u32 run_mode;
    chip8_u32 max_frames;
    chip8_u64 max_cycles;
    chip8_u32 cycles_per_frame;
    chip8_u32 input_frame_count;
    chip8_u32 input_stride;
    chip8_u32 frame_hash_capacity;
    chip8_u32 frame_hash_stride;
    chip8_u32 quirk_flags;
    chip8_u32 reserved;
    // Makes the 8-byte-aligned record's final four bytes explicit so the host
    // never transmits indeterminate tail padding in this by-value argument.
    chip8_u32 reserved_tail;
};

// Complete scalar CPU state.  Memory and the two physical display planes are
// deliberately external because their transposed representation is part of
// the ABI.  `hires` and `halted` use 0/1 byte representations.  A zero RNG
// state is normalized to one, matching Rng::new/from_state in the CPU core.
struct Chip8CudaLaneState {
    chip8_u64 rng_state;
    chip8_u16 i;
    chip8_u16 pc;
    chip8_u16 stack[16];
    chip8_u16 keys;
    chip8_u8 v[16];
    chip8_u8 flags[16];
    chip8_u8 audio_buf[16];
    chip8_u8 sp;
    chip8_u8 dt;
    chip8_u8 st;
    chip8_u8 hires;
    chip8_u8 plane;
    chip8_u8 halted;
    chip8_u8 key_wait_kind;
    chip8_u8 key_wait_reg;
    chip8_u8 key_wait_key;
    chip8_u8 audio_pitch;
};

// `fault_address` is 65536 for every current out-of-range memory operation,
// exactly matching the CPU family.  It is zero for non-memory terminations.
// `final_frame_hash` is always populated, including when no frame completed.
struct Chip8CudaLaneResult {
    chip8_u32 abi_version;
    chip8_u32 status;
    chip8_u64 cycles_executed;
    chip8_u32 frames_executed;
    chip8_u32 frame_hashes_written;
    chip8_u32 fault_address;
    chip8_u16 invalid_opcode;
    chip8_u16 reserved;
    chip8_u64 final_frame_hash;
    chip8_u32 draw_count;
    chip8_u32 collision_count;
    chip8_u32 input_opcode_count;
    chip8_u32 clear_count;
    chip8_u32 delay_timer_set_count;
    chip8_u32 delay_timer_nonzero_count;
    chip8_u32 sound_timer_set_count;
    chip8_u32 sound_timer_nonzero_count;
    chip8_u32 scroll_count;
    chip8_u32 opcode_counts[16];
    // Occupies the alignment gap before final_state so every result byte is
    // initialized and ABI-versioned rather than implicit device-side padding.
    chip8_u32 reserved_counts;
    Chip8CudaLaneState final_state;
};

static_assert(sizeof(Chip8CudaConfig) == 56,
              "Chip8CudaConfig v1 layout changed");
static_assert(alignof(Chip8CudaConfig) == 8,
              "Chip8CudaConfig v1 alignment changed");
static_assert(sizeof(Chip8CudaLaneState) == 104,
              "Chip8CudaLaneState v1 layout changed");
static_assert(alignof(Chip8CudaLaneState) == 8,
              "Chip8CudaLaneState v1 alignment changed");
static_assert(sizeof(Chip8CudaLaneResult) == 248,
              "Chip8CudaLaneResult v1 layout changed");
static_assert(alignof(Chip8CudaLaneResult) == 8,
              "Chip8CudaLaneResult v1 alignment changed");

namespace {

enum StepStatus {
    STEP_OK = 0,
    STEP_HALTED = 1,
    STEP_WAITING_FOR_KEY = 2,
    STEP_INVALID_OPCODE = 3,
    STEP_STACK_OVERFLOW = 4,
    STEP_STACK_UNDERFLOW = 5,
    STEP_MEMORY_FAULT = 6,
};

struct LaneContext {
    chip8_u32 lane;
    chip8_u32 lane_count;
    chip8_u32 quirks;
    chip8_u8 *memory;
    chip8_u8 *display;
    Chip8CudaLaneState state;
    chip8_u16 invalid_opcode;
    chip8_u32 fault_address;
    chip8_u32 draw_count;
    chip8_u32 collision_count;
    chip8_u32 input_opcode_count;
    chip8_u32 clear_count;
    chip8_u32 delay_timer_set_count;
    chip8_u32 delay_timer_nonzero_count;
    chip8_u32 sound_timer_set_count;
    chip8_u32 sound_timer_nonzero_count;
    chip8_u32 scroll_count;
    chip8_u32 opcode_counts[16];
};

__device__ __forceinline__ void saturating_increment(chip8_u32 &value) {
    if (value != 0xFFFFFFFFu) {
        ++value;
    }
}

__device__ __forceinline__ chip8_u64 memory_offset(const LaneContext &ctx,
                                                    chip8_u32 address) {
    return static_cast<chip8_u64>(address) * ctx.lane_count + ctx.lane;
}

__device__ __forceinline__ chip8_u8 memory_read(const LaneContext &ctx,
                                                chip8_u16 address) {
    return ctx.memory[memory_offset(ctx, static_cast<chip8_u32>(address))];
}

__device__ __forceinline__ void memory_write(LaneContext &ctx,
                                             chip8_u16 address,
                                             chip8_u8 value) {
    ctx.memory[memory_offset(ctx, static_cast<chip8_u32>(address))] = value;
}

__device__ __forceinline__ chip8_u64 display_offset(const LaneContext &ctx,
                                                     chip8_u32 plane,
                                                     chip8_u32 pixel) {
    const chip8_u32 plane_pixel = plane * CHIP8_DISPLAY_PLANE_SIZE + pixel;
    return static_cast<chip8_u64>(plane_pixel) * ctx.lane_count + ctx.lane;
}

__device__ __forceinline__ chip8_u8 display_read_physical(
    const LaneContext &ctx, chip8_u32 plane, chip8_u32 pixel) {
    return ctx.display[display_offset(ctx, plane, pixel)];
}

__device__ __forceinline__ void display_write_physical(
    LaneContext &ctx, chip8_u32 plane, chip8_u32 pixel, chip8_u8 value) {
    ctx.display[display_offset(ctx, plane, pixel)] = value;
}

__device__ __forceinline__ chip8_u32 display_width(const LaneContext &ctx) {
    return ctx.state.hires != 0 ? 128u : 64u;
}

__device__ __forceinline__ chip8_u32 display_height(const LaneContext &ctx) {
    return ctx.state.hires != 0 ? 64u : 32u;
}

__device__ __forceinline__ chip8_u32 display_logical_index(
    const LaneContext &ctx, chip8_u32 x, chip8_u32 y) {
    if (ctx.state.hires != 0) {
        return y * 128u + x;
    }
    return (y * 2u) * 128u + x * 2u;
}

__device__ __forceinline__ chip8_u8 display_get(const LaneContext &ctx,
                                                chip8_u32 plane,
                                                chip8_u32 x,
                                                chip8_u32 y) {
    return display_read_physical(ctx, plane,
                                 display_logical_index(ctx, x, y));
}

__device__ __forceinline__ void display_set(LaneContext &ctx,
                                            chip8_u32 plane,
                                            chip8_u32 x,
                                            chip8_u32 y,
                                            chip8_u8 value) {
    const chip8_u32 pixel = display_logical_index(ctx, x, y);
    display_write_physical(ctx, plane, pixel, value);
    if (ctx.state.hires == 0) {
        display_write_physical(ctx, plane, pixel + 1u, value);
        display_write_physical(ctx, plane, pixel + 128u, value);
        display_write_physical(ctx, plane, pixel + 129u, value);
    }
}

__device__ void display_clear_selected(LaneContext &ctx) {
    for (chip8_u32 plane = 0; plane < 2; ++plane) {
        if ((ctx.state.plane & static_cast<chip8_u8>(1u << plane)) == 0) {
            continue;
        }
        for (chip8_u32 pixel = 0; pixel < CHIP8_DISPLAY_PLANE_SIZE;
             ++pixel) {
            display_write_physical(ctx, plane, pixel, 0);
        }
    }
}

__device__ void display_clear_all(LaneContext &ctx) {
    for (chip8_u32 plane = 0; plane < 2; ++plane) {
        for (chip8_u32 pixel = 0; pixel < CHIP8_DISPLAY_PLANE_SIZE;
             ++pixel) {
            display_write_physical(ctx, plane, pixel, 0);
        }
    }
}

__device__ void display_scroll_down(LaneContext &ctx, chip8_u32 rows) {
    const chip8_u32 width = display_width(ctx);
    const chip8_u32 height = display_height(ctx);
    for (chip8_u32 plane = 0; plane < 2; ++plane) {
        if ((ctx.state.plane & static_cast<chip8_u8>(1u << plane)) == 0) {
            continue;
        }
        for (chip8_u32 row = height; row > rows; --row) {
            const chip8_u32 target_row = row - 1u;
            const chip8_u32 source_row = target_row - rows;
            for (chip8_u32 column = 0; column < width; ++column) {
                const chip8_u8 value =
                    display_get(ctx, plane, column, source_row);
                display_set(ctx, plane, column, target_row, value);
            }
        }
        for (chip8_u32 row = 0; row < rows; ++row) {
            for (chip8_u32 column = 0; column < width; ++column) {
                display_set(ctx, plane, column, row, 0);
            }
        }
    }
}

__device__ void display_scroll_up(LaneContext &ctx, chip8_u32 rows) {
    const chip8_u32 width = display_width(ctx);
    const chip8_u32 height = display_height(ctx);
    const chip8_u32 retained_rows = rows < height ? height - rows : 0u;
    for (chip8_u32 plane = 0; plane < 2; ++plane) {
        if ((ctx.state.plane & static_cast<chip8_u8>(1u << plane)) == 0) {
            continue;
        }
        for (chip8_u32 row = 0; row < retained_rows; ++row) {
            for (chip8_u32 column = 0; column < width; ++column) {
                const chip8_u8 value =
                    display_get(ctx, plane, column, row + rows);
                display_set(ctx, plane, column, row, value);
            }
        }
        for (chip8_u32 row = retained_rows; row < height; ++row) {
            for (chip8_u32 column = 0; column < width; ++column) {
                display_set(ctx, plane, column, row, 0);
            }
        }
    }
}

__device__ void display_scroll_right(LaneContext &ctx) {
    const chip8_u32 width = display_width(ctx);
    const chip8_u32 height = display_height(ctx);
    const chip8_u32 shift = 4u;
    for (chip8_u32 plane = 0; plane < 2; ++plane) {
        if ((ctx.state.plane & static_cast<chip8_u8>(1u << plane)) == 0) {
            continue;
        }
        for (chip8_u32 row = 0; row < height; ++row) {
            for (chip8_u32 column = width; column > shift; --column) {
                const chip8_u32 target_column = column - 1u;
                const chip8_u8 value =
                    display_get(ctx, plane, target_column - shift, row);
                display_set(ctx, plane, target_column, row, value);
            }
            for (chip8_u32 column = 0; column < shift; ++column) {
                display_set(ctx, plane, column, row, 0);
            }
        }
    }
}

__device__ void display_scroll_left(LaneContext &ctx) {
    const chip8_u32 width = display_width(ctx);
    const chip8_u32 height = display_height(ctx);
    const chip8_u32 shift = 4u;
    for (chip8_u32 plane = 0; plane < 2; ++plane) {
        if ((ctx.state.plane & static_cast<chip8_u8>(1u << plane)) == 0) {
            continue;
        }
        for (chip8_u32 row = 0; row < height; ++row) {
            for (chip8_u32 column = 0; column < width - shift; ++column) {
                const chip8_u8 value =
                    display_get(ctx, plane, column + shift, row);
                display_set(ctx, plane, column, row, value);
            }
            for (chip8_u32 column = width - shift; column < width; ++column) {
                display_set(ctx, plane, column, row, 0);
            }
        }
    }
}

// Mirrors Display::xor_sprite_byte.  In particular, horizontal overflow wraps
// only when clipping is disabled while rows below the bottom edge are always
// discarded by the CPU implementation.
__device__ bool display_xor_sprite_byte(LaneContext &ctx,
                                        chip8_u32 plane,
                                        chip8_u32 x,
                                        chip8_u32 y,
                                        chip8_u8 byte,
                                        bool clipping) {
    const chip8_u32 width = display_width(ctx);
    const chip8_u32 height = display_height(ctx);
    bool collision = false;
    for (chip8_u32 bit = 0; bit < 8; ++bit) {
        chip8_u32 pixel_x = x + bit;
        if (pixel_x >= width) {
            if (clipping) {
                continue;
            }
            pixel_x %= width;
        }
        const chip8_u8 bit_value =
            static_cast<chip8_u8>((byte >> (7u - bit)) & 1u);
        if (bit_value == 0 || y >= height) {
            continue;
        }
        const chip8_u8 old_value = display_get(ctx, plane, pixel_x, y);
        const chip8_u8 new_value =
            static_cast<chip8_u8>(old_value ^ bit_value);
        display_set(ctx, plane, pixel_x, y, new_value);
        if (old_value == 1 && new_value == 0) {
            collision = true;
        }
    }
    return collision;
}

__device__ __forceinline__ chip8_u32 selected_plane_count(chip8_u8 plane) {
    const chip8_u8 mask = static_cast<chip8_u8>(plane & 0x3u);
    return static_cast<chip8_u32>(mask & 1u) +
           static_cast<chip8_u32>((mask >> 1u) & 1u);
}

__device__ StepStatus draw_sprite(LaneContext &ctx,
                                  chip8_u32 x,
                                  chip8_u32 y,
                                  chip8_u8 n) {
    const chip8_u32 width = display_width(ctx);
    const chip8_u32 height = display_height(ctx);
    const chip8_u32 start_x = static_cast<chip8_u32>(ctx.state.v[x]) % width;
    const chip8_u32 start_y = static_cast<chip8_u32>(ctx.state.v[y]) % height;
    const bool clipping = (ctx.quirks & CHIP8_QUIRK_CLIPPING) != 0;
    const bool is_16_by_16 = n == 0;
    const chip8_u32 bytes_per_plane =
        is_16_by_16 ? 32u : static_cast<chip8_u32>(n);
    const chip8_u32 byte_count =
        bytes_per_plane * selected_plane_count(ctx.state.plane);
    const chip8_u32 base = static_cast<chip8_u32>(ctx.state.i);

    if (byte_count != 0 && base + byte_count - 1u >= CHIP8_MEMORY_SIZE) {
        ctx.fault_address = CHIP8_MEMORY_SIZE;
        return STEP_MEMORY_FAULT;
    }

    bool collision = false;
    chip8_u32 selected_index = 0;
    const chip8_u8 plane_mask = ctx.state.plane;
    for (chip8_u32 plane = 0; plane < 2; ++plane) {
        if ((plane_mask & static_cast<chip8_u8>(1u << plane)) == 0) {
            continue;
        }
        const chip8_u32 plane_base = base + selected_index * bytes_per_plane;
        ++selected_index;
        const chip8_u32 rows = is_16_by_16 ? 16u : n;
        for (chip8_u32 row = 0; row < rows; ++row) {
            const chip8_u32 pixel_y = start_y + row;
            if (is_16_by_16) {
                const chip8_u8 first = memory_read(
                    ctx, static_cast<chip8_u16>(plane_base + row * 2u));
                const chip8_u8 second = memory_read(
                    ctx, static_cast<chip8_u16>(plane_base + row * 2u + 1u));
                if (pixel_y >= height && clipping) {
                    continue;
                }
                if (display_xor_sprite_byte(ctx, plane, start_x, pixel_y,
                                            first, clipping)) {
                    collision = true;
                }
                if (display_xor_sprite_byte(ctx, plane, start_x + 8u, pixel_y,
                                            second, clipping)) {
                    collision = true;
                }
            } else {
                const chip8_u8 byte = memory_read(
                    ctx, static_cast<chip8_u16>(plane_base + row));
                if (display_xor_sprite_byte(ctx, plane, start_x, pixel_y,
                                            byte, clipping)) {
                    collision = true;
                }
            }
        }
    }
    ctx.state.v[15] = collision ? 1 : 0;
    saturating_increment(ctx.draw_count);
    if (collision) {
        saturating_increment(ctx.collision_count);
    }
    return STEP_OK;
}

__device__ __forceinline__ bool key_is_pressed(const LaneContext &ctx,
                                               chip8_u8 key) {
    return key < 16 &&
           ((ctx.state.keys >> static_cast<chip8_u32>(key)) & 1u) != 0;
}

__device__ __forceinline__ chip8_u8 first_pressed_key(
    const LaneContext &ctx) {
    for (chip8_u8 key = 0; key < 16; ++key) {
        if (key_is_pressed(ctx, key)) {
            return key;
        }
    }
    return 0xFFu;
}

__device__ __forceinline__ chip8_u8 random_byte(LaneContext &ctx) {
    chip8_u64 state = ctx.state.rng_state;
    state ^= state << 13u;
    state ^= state >> 7u;
    state ^= state << 17u;
    ctx.state.rng_state = state;
    return static_cast<chip8_u8>(state & 0xFFu);
}

__device__ __forceinline__ chip8_u16 peek_opcode(const LaneContext &ctx) {
    const chip8_u16 pc = ctx.state.pc;
    const chip8_u8 high = memory_read(ctx, pc);
    const chip8_u8 low =
        memory_read(ctx, static_cast<chip8_u16>(pc + static_cast<chip8_u16>(1)));
    return static_cast<chip8_u16>((static_cast<chip8_u16>(high) << 8u) |
                                  static_cast<chip8_u16>(low));
}

__device__ __forceinline__ void record_opcode_attempt(LaneContext &ctx) {
    const chip8_u16 opcode = peek_opcode(ctx);
    const chip8_u32 opcode_class = (opcode >> 12u) & 0x0Fu;
    ++ctx.opcode_counts[opcode_class];
}

__device__ __forceinline__ chip8_u16 fetch_opcode(LaneContext &ctx) {
    const chip8_u16 pc = ctx.state.pc;
    const chip8_u8 high = memory_read(ctx, pc);
    const chip8_u8 low =
        memory_read(ctx, static_cast<chip8_u16>(pc + static_cast<chip8_u16>(1)));
    ctx.state.pc = static_cast<chip8_u16>(pc + static_cast<chip8_u16>(2));
    return static_cast<chip8_u16>((static_cast<chip8_u16>(high) << 8u) |
                                  static_cast<chip8_u16>(low));
}

__device__ __forceinline__ StepStatus invalid_opcode(LaneContext &ctx,
                                                     chip8_u16 opcode) {
    ctx.invalid_opcode = opcode;
    return STEP_INVALID_OPCODE;
}

__device__ StepStatus execute_step(LaneContext &ctx) {
    if (ctx.state.halted != 0) {
        return STEP_HALTED;
    }

    if (ctx.state.key_wait_kind == CHIP8_KEY_WAIT_PRESS) {
        const chip8_u8 key = first_pressed_key(ctx);
        if (key != 0xFFu) {
            ctx.state.key_wait_kind = CHIP8_KEY_WAIT_RELEASE;
            ctx.state.key_wait_key = key;
        }
        return STEP_WAITING_FOR_KEY;
    }
    if (ctx.state.key_wait_kind == CHIP8_KEY_WAIT_RELEASE) {
        const chip8_u8 key = ctx.state.key_wait_key;
        if (!key_is_pressed(ctx, key)) {
            ctx.state.v[ctx.state.key_wait_reg & 0x0Fu] = key;
            ctx.state.key_wait_kind = CHIP8_KEY_WAIT_NONE;
            // EventCounts treats both KeyWaitEntered and KeyWaitResolved as
            // input opcodes; preserve that existing (slightly broader than
            // its field documentation) CPU behavior.
            saturating_increment(ctx.input_opcode_count);
            return STEP_OK;
        }
        return STEP_WAITING_FOR_KEY;
    }

    const chip8_u16 opcode = fetch_opcode(ctx);
    const chip8_u32 top = (opcode >> 12u) & 0x0Fu;
    const chip8_u32 x = (opcode >> 8u) & 0x0Fu;
    const chip8_u32 y = (opcode >> 4u) & 0x0Fu;
    const chip8_u8 n = static_cast<chip8_u8>(opcode & 0x0Fu);
    const chip8_u8 nn = static_cast<chip8_u8>(opcode & 0x00FFu);
    const chip8_u16 nnn = static_cast<chip8_u16>(opcode & 0x0FFFu);

    switch (top) {
        case 0x0u:
            if (opcode == 0x00E0u) {
                display_clear_selected(ctx);
                saturating_increment(ctx.clear_count);
                return STEP_OK;
            }
            if (opcode == 0x00EEu) {
                if (ctx.state.sp == 0) {
                    return STEP_STACK_UNDERFLOW;
                }
                --ctx.state.sp;
                ctx.state.pc = ctx.state.stack[ctx.state.sp];
                return STEP_OK;
            }
            if (opcode == 0x00FDu) {
                ctx.state.halted = 1;
                return STEP_HALTED;
            }
            if (opcode == 0x00FEu) {
                ctx.state.hires = 0;
                display_clear_all(ctx);
                return STEP_OK;
            }
            if (opcode == 0x00FFu) {
                ctx.state.hires = 1;
                display_clear_all(ctx);
                return STEP_OK;
            }
            if (opcode == 0x00FBu) {
                display_scroll_right(ctx);
                saturating_increment(ctx.scroll_count);
                return STEP_OK;
            }
            if (opcode == 0x00FCu) {
                display_scroll_left(ctx);
                saturating_increment(ctx.scroll_count);
                return STEP_OK;
            }
            if ((opcode & 0xFFF0u) == 0x00C0u) {
                display_scroll_down(ctx, n);
                saturating_increment(ctx.scroll_count);
                return STEP_OK;
            }
            if ((opcode & 0xFFF0u) == 0x00D0u) {
                display_scroll_up(ctx, n);
                saturating_increment(ctx.scroll_count);
                return STEP_OK;
            }
            // 0NNN is a no-op on the modern CPU implementation.
            return STEP_OK;

        case 0x1u:
            ctx.state.pc = nnn;
            return STEP_OK;

        case 0x2u:
            if (ctx.state.sp >= 16) {
                return STEP_STACK_OVERFLOW;
            }
            ctx.state.stack[ctx.state.sp] = ctx.state.pc;
            ++ctx.state.sp;
            ctx.state.pc = nnn;
            return STEP_OK;

        case 0x3u:
            if (ctx.state.v[x] == nn) {
                ctx.state.pc = static_cast<chip8_u16>(ctx.state.pc + 2u);
            }
            return STEP_OK;

        case 0x4u:
            if (ctx.state.v[x] != nn) {
                ctx.state.pc = static_cast<chip8_u16>(ctx.state.pc + 2u);
            }
            return STEP_OK;

        case 0x5u:
            if (n == 0) {
                if (ctx.state.v[x] == ctx.state.v[y]) {
                    ctx.state.pc = static_cast<chip8_u16>(ctx.state.pc + 2u);
                }
                return STEP_OK;
            }
            if (n == 2u || n == 3u) {
                const chip8_u32 length = x >= y ? x - y + 1u : y - x + 1u;
                const chip8_u32 base = ctx.state.i;
                if (base + length - 1u >= CHIP8_MEMORY_SIZE) {
                    ctx.fault_address = CHIP8_MEMORY_SIZE;
                    return STEP_MEMORY_FAULT;
                }
                for (chip8_u32 offset = 0; offset < length; ++offset) {
                    const chip8_u32 reg = x <= y ? x + offset : x - offset;
                    const chip8_u16 address =
                        static_cast<chip8_u16>(base + offset);
                    if (n == 2u) {
                        memory_write(ctx, address, ctx.state.v[reg]);
                    } else {
                        ctx.state.v[reg] = memory_read(ctx, address);
                    }
                }
                return STEP_OK;
            }
            return invalid_opcode(ctx, opcode);

        case 0x6u:
            ctx.state.v[x] = nn;
            return STEP_OK;

        case 0x7u:
            ctx.state.v[x] = static_cast<chip8_u8>(ctx.state.v[x] + nn);
            return STEP_OK;

        case 0x8u:
            switch (n) {
                case 0x0u:
                    ctx.state.v[x] = ctx.state.v[y];
                    return STEP_OK;
                case 0x1u:
                    ctx.state.v[x] =
                        static_cast<chip8_u8>(ctx.state.v[x] | ctx.state.v[y]);
                    if ((ctx.quirks & CHIP8_QUIRK_VF_RESET_ON_LOGIC) != 0) {
                        ctx.state.v[15] = 0;
                    }
                    return STEP_OK;
                case 0x2u:
                    ctx.state.v[x] =
                        static_cast<chip8_u8>(ctx.state.v[x] & ctx.state.v[y]);
                    if ((ctx.quirks & CHIP8_QUIRK_VF_RESET_ON_LOGIC) != 0) {
                        ctx.state.v[15] = 0;
                    }
                    return STEP_OK;
                case 0x3u:
                    ctx.state.v[x] =
                        static_cast<chip8_u8>(ctx.state.v[x] ^ ctx.state.v[y]);
                    if ((ctx.quirks & CHIP8_QUIRK_VF_RESET_ON_LOGIC) != 0) {
                        ctx.state.v[15] = 0;
                    }
                    return STEP_OK;
                case 0x4u: {
                    const chip8_u16 sum = static_cast<chip8_u16>(ctx.state.v[x]) +
                                          static_cast<chip8_u16>(ctx.state.v[y]);
                    ctx.state.v[x] = static_cast<chip8_u8>(sum);
                    ctx.state.v[15] = sum > 0xFFu ? 1 : 0;
                    return STEP_OK;
                }
                case 0x5u: {
                    const chip8_u8 left = ctx.state.v[x];
                    const chip8_u8 right = ctx.state.v[y];
                    ctx.state.v[x] = static_cast<chip8_u8>(left - right);
                    ctx.state.v[15] = left >= right ? 1 : 0;
                    return STEP_OK;
                }
                case 0x6u: {
                    const chip8_u8 source =
                        (ctx.quirks & CHIP8_QUIRK_SHIFT_VX_ONLY) != 0
                            ? ctx.state.v[x]
                            : ctx.state.v[y];
                    ctx.state.v[15] = static_cast<chip8_u8>(source & 1u);
                    ctx.state.v[x] = static_cast<chip8_u8>(source >> 1u);
                    return STEP_OK;
                }
                case 0x7u: {
                    const chip8_u8 left = ctx.state.v[y];
                    const chip8_u8 right = ctx.state.v[x];
                    ctx.state.v[x] = static_cast<chip8_u8>(left - right);
                    ctx.state.v[15] = left >= right ? 1 : 0;
                    return STEP_OK;
                }
                case 0xEu: {
                    const chip8_u8 source =
                        (ctx.quirks & CHIP8_QUIRK_SHIFT_VX_ONLY) != 0
                            ? ctx.state.v[x]
                            : ctx.state.v[y];
                    ctx.state.v[15] =
                        static_cast<chip8_u8>((source >> 7u) & 1u);
                    ctx.state.v[x] = static_cast<chip8_u8>(source << 1u);
                    return STEP_OK;
                }
                default:
                    return invalid_opcode(ctx, opcode);
            }

        case 0x9u:
            if (n != 0) {
                return invalid_opcode(ctx, opcode);
            }
            if (ctx.state.v[x] != ctx.state.v[y]) {
                ctx.state.pc = static_cast<chip8_u16>(ctx.state.pc + 2u);
            }
            return STEP_OK;

        case 0xAu:
            ctx.state.i = nnn;
            return STEP_OK;

        case 0xBu: {
            const chip8_u16 offset =
                (ctx.quirks & CHIP8_QUIRK_JUMP_OFFSET_VX) != 0
                    ? static_cast<chip8_u16>(ctx.state.v[x])
                    : static_cast<chip8_u16>(ctx.state.v[0]);
            ctx.state.pc = static_cast<chip8_u16>(nnn + offset);
            return STEP_OK;
        }

        case 0xCu:
            ctx.state.v[x] = static_cast<chip8_u8>(random_byte(ctx) & nn);
            return STEP_OK;

        case 0xDu:
            return draw_sprite(ctx, x, y, n);

        case 0xEu:
            if (nn == 0x9Eu) {
                if (key_is_pressed(ctx, ctx.state.v[x])) {
                    ctx.state.pc = static_cast<chip8_u16>(ctx.state.pc + 2u);
                }
                return STEP_OK;
            }
            if (nn == 0xA1u) {
                if (!key_is_pressed(ctx, ctx.state.v[x])) {
                    ctx.state.pc = static_cast<chip8_u16>(ctx.state.pc + 2u);
                }
                return STEP_OK;
            }
            return invalid_opcode(ctx, opcode);

        case 0xFu:
            if (nn == 0x07u) {
                ctx.state.v[x] = ctx.state.dt;
                return STEP_OK;
            }
            if (nn == 0x0Au) {
                ctx.state.key_wait_kind = CHIP8_KEY_WAIT_PRESS;
                ctx.state.key_wait_reg = static_cast<chip8_u8>(x);
                saturating_increment(ctx.input_opcode_count);
                return STEP_OK;
            }
            if (nn == 0x15u) {
                ctx.state.dt = ctx.state.v[x];
                saturating_increment(ctx.delay_timer_set_count);
                if (ctx.state.v[x] != 0) {
                    saturating_increment(ctx.delay_timer_nonzero_count);
                }
                return STEP_OK;
            }
            if (nn == 0x18u) {
                ctx.state.st = ctx.state.v[x];
                saturating_increment(ctx.sound_timer_set_count);
                if (ctx.state.v[x] != 0) {
                    saturating_increment(ctx.sound_timer_nonzero_count);
                }
                return STEP_OK;
            }
            if (nn == 0x1Eu) {
                ctx.state.i = static_cast<chip8_u16>(
                    ctx.state.i + static_cast<chip8_u16>(ctx.state.v[x]));
                return STEP_OK;
            }
            if (nn == 0x29u) {
                const chip8_u16 digit =
                    static_cast<chip8_u16>(ctx.state.v[x] & 0x0Fu);
                ctx.state.i = static_cast<chip8_u16>(0x0050u + digit * 5u);
                return STEP_OK;
            }
            if (nn == 0x30u) {
                const chip8_u16 digit =
                    static_cast<chip8_u16>(ctx.state.v[x] % 10u);
                ctx.state.i = static_cast<chip8_u16>(0x0100u + digit * 10u);
                return STEP_OK;
            }
            if (nn == 0x33u) {
                const chip8_u32 base = ctx.state.i;
                if (base + 2u >= CHIP8_MEMORY_SIZE) {
                    ctx.fault_address = CHIP8_MEMORY_SIZE;
                    return STEP_MEMORY_FAULT;
                }
                const chip8_u8 value = ctx.state.v[x];
                memory_write(ctx, static_cast<chip8_u16>(base),
                             static_cast<chip8_u8>(value / 100u));
                memory_write(ctx, static_cast<chip8_u16>(base + 1u),
                             static_cast<chip8_u8>((value / 10u) % 10u));
                memory_write(ctx, static_cast<chip8_u16>(base + 2u),
                             static_cast<chip8_u8>(value % 10u));
                return STEP_OK;
            }
            if (nn == 0x3Au) {
                ctx.state.audio_pitch = ctx.state.v[x];
                return STEP_OK;
            }
            if (nn == 0x55u || nn == 0x65u) {
                const chip8_u32 base = ctx.state.i;
                if (base + x >= CHIP8_MEMORY_SIZE) {
                    ctx.fault_address = CHIP8_MEMORY_SIZE;
                    return STEP_MEMORY_FAULT;
                }
                for (chip8_u32 reg = 0; reg <= x; ++reg) {
                    const chip8_u16 address =
                        static_cast<chip8_u16>(base + reg);
                    if (nn == 0x55u) {
                        memory_write(ctx, address, ctx.state.v[reg]);
                    } else {
                        ctx.state.v[reg] = memory_read(ctx, address);
                    }
                }
                if ((ctx.quirks & CHIP8_QUIRK_LOAD_STORE_NO_INC_I) == 0) {
                    ctx.state.i = static_cast<chip8_u16>(
                        ctx.state.i + static_cast<chip8_u16>(x + 1u));
                }
                return STEP_OK;
            }
            if (nn == 0x75u) {
                for (chip8_u32 reg = 0; reg <= x; ++reg) {
                    ctx.state.flags[reg] = ctx.state.v[reg];
                }
                return STEP_OK;
            }
            if (nn == 0x85u) {
                for (chip8_u32 reg = 0; reg <= x; ++reg) {
                    ctx.state.v[reg] = ctx.state.flags[reg];
                }
                return STEP_OK;
            }
            if (opcode == 0xF000u) {
                const chip8_u16 operand_pc = ctx.state.pc;
                const chip8_u16 next_pc =
                    static_cast<chip8_u16>(operand_pc + 1u);
                const chip8_u16 high = memory_read(ctx, operand_pc);
                const chip8_u16 low = memory_read(ctx, next_pc);
                ctx.state.pc = static_cast<chip8_u16>(ctx.state.pc + 2u);
                ctx.state.i = static_cast<chip8_u16>((high << 8u) | low);
                return STEP_OK;
            }
            if (nn == 0x01u) {
                ctx.state.plane = static_cast<chip8_u8>(x & 0x03u);
                return STEP_OK;
            }
            if (opcode == 0xF002u) {
                const chip8_u32 base = ctx.state.i;
                if (base + 16u > CHIP8_MEMORY_SIZE) {
                    ctx.fault_address = CHIP8_MEMORY_SIZE;
                    return STEP_MEMORY_FAULT;
                }
                for (chip8_u32 index = 0; index < 16; ++index) {
                    ctx.state.audio_buf[index] = memory_read(
                        ctx, static_cast<chip8_u16>(base + index));
                }
                return STEP_OK;
            }
            return invalid_opcode(ctx, opcode);

        default:
            return invalid_opcode(ctx, opcode);
    }
}

__device__ chip8_u64 frame_hash(const LaneContext &ctx) {
    chip8_u64 hash = 14695981039346656037ull;
    for (chip8_u32 plane = 0; plane < 2; ++plane) {
        for (chip8_u32 pixel = 0; pixel < CHIP8_DISPLAY_PLANE_SIZE;
             ++pixel) {
            hash ^= static_cast<chip8_u64>(
                display_read_physical(ctx, plane, pixel));
            hash *= 1099511628211ull;
        }
    }
    return hash;
}

__device__ __forceinline__ chip8_u32 public_status(StepStatus status) {
    switch (status) {
        case STEP_HALTED:
            return CHIP8_STATUS_HALTED;
        case STEP_INVALID_OPCODE:
            return CHIP8_STATUS_INVALID_OPCODE;
        case STEP_STACK_OVERFLOW:
            return CHIP8_STATUS_STACK_OVERFLOW;
        case STEP_STACK_UNDERFLOW:
            return CHIP8_STATUS_STACK_UNDERFLOW;
        case STEP_MEMORY_FAULT:
            return CHIP8_STATUS_MEMORY_FAULT;
        default:
            return CHIP8_STATUS_TIMEOUT;
    }
}

__device__ __forceinline__ void publish_result(
    const LaneContext &ctx,
    chip8_u32 status,
    chip8_u64 cycles,
    chip8_u32 frames,
    chip8_u32 hashes_written,
    chip8_u64 final_hash,
    Chip8CudaLaneResult &result) {
    result.abi_version = CHIP8_CUDA_ABI_VERSION;
    result.status = status;
    result.cycles_executed = cycles;
    result.frames_executed = frames;
    result.frame_hashes_written = hashes_written;
    result.fault_address = ctx.fault_address;
    result.invalid_opcode = ctx.invalid_opcode;
    result.reserved = 0;
    result.final_frame_hash = final_hash;
    result.draw_count = ctx.draw_count;
    result.collision_count = ctx.collision_count;
    result.input_opcode_count = ctx.input_opcode_count;
    result.clear_count = ctx.clear_count;
    result.delay_timer_set_count = ctx.delay_timer_set_count;
    result.delay_timer_nonzero_count = ctx.delay_timer_nonzero_count;
    result.sound_timer_set_count = ctx.sound_timer_set_count;
    result.sound_timer_nonzero_count = ctx.sound_timer_nonzero_count;
    result.scroll_count = ctx.scroll_count;
    for (chip8_u32 opcode_class = 0; opcode_class < 16; ++opcode_class) {
        result.opcode_counts[opcode_class] = ctx.opcode_counts[opcode_class];
    }
    result.reserved_counts = 0;
    result.final_state = ctx.state;
}

}  // namespace

// One independent virtual machine is executed by each CUDA thread.  The host
// must launch a one-dimensional grid containing at least `lane_count` threads;
// excess threads return immediately.  No shared memory or synchronization is
// required.
//
// Required non-null device buffers:
//   initial_states: lane_count Chip8CudaLaneState values
//   memory_soa:     65536 * lane_count bytes, input and final output
//   display_soa:    2 * 8192 * lane_count bytes, input and final output
//   results:        lane_count Chip8CudaLaneResult values
//
// input_masks may be null only when input_frame_count is zero.  frame_hashes
// may be null only when frame_hash_capacity is zero.  In frame mode, a frame
// is completed only after all cycles execute normally or wait for a key; only
// completed frames decrement timers and produce a hash.  Cycle mode requires
// both optional counts to be zero and performs exactly the run_cycles hot path.
//
// Results are launch-local deltas and the large buffers are updated in place.
// Feeding `final_state` into a subsequent launch therefore gives the host an
// exact resumable primitive for policy boundaries such as UntilStagnant.  The
// host accumulates integer counters and stops before launching an extra frame;
// this kernel deliberately performs no floating-point policy approximation.
extern "C" __global__ void chip8_cuda_run_v1(
    Chip8CudaConfig config,
    const Chip8CudaLaneState *initial_states,
    chip8_u8 *memory_soa,
    chip8_u8 *display_soa,
    const chip8_u16 *input_masks,
    chip8_u64 *frame_hashes,
    Chip8CudaLaneResult *results) {
    const chip8_u64 global_lane =
        static_cast<chip8_u64>(blockIdx.x) * blockDim.x + threadIdx.x;
    if (global_lane >= config.lane_count || results == 0) {
        return;
    }
    const chip8_u32 lane = static_cast<chip8_u32>(global_lane);

    LaneContext ctx = {};
    ctx.lane = lane;
    ctx.lane_count = config.lane_count;
    ctx.quirks = config.quirk_flags;
    ctx.memory = memory_soa;
    ctx.display = display_soa;

    // Keep invalid-configuration reporting deterministic even if the initial
    // state pointer itself is missing.
    Chip8CudaLaneState zero_state = {};
    ctx.state = initial_states != 0 ? initial_states[lane] : zero_state;

    const bool invalid_configuration =
        config.abi_version != CHIP8_CUDA_ABI_VERSION ||
        config.reserved != 0 || config.reserved_tail != 0 ||
        config.run_mode > CHIP8_RUN_CYCLES ||
        (config.quirk_flags & ~static_cast<chip8_u32>(CHIP8_QUIRK_MASK)) != 0 ||
        initial_states == 0 || memory_soa == 0 || display_soa == 0 ||
        (config.input_frame_count != 0 && input_masks == 0) ||
        config.input_stride < config.input_frame_count ||
        (config.frame_hash_capacity != 0 && frame_hashes == 0) ||
        config.frame_hash_stride < config.frame_hash_capacity ||
        (config.run_mode == CHIP8_RUN_CYCLES &&
         (config.input_frame_count != 0 ||
          config.frame_hash_capacity != 0)) ||
        ctx.state.sp > 16 || ctx.state.key_wait_kind > CHIP8_KEY_WAIT_RELEASE ||
        (ctx.state.key_wait_kind != CHIP8_KEY_WAIT_NONE &&
         ctx.state.key_wait_reg >= 16) ||
        (ctx.state.key_wait_kind == CHIP8_KEY_WAIT_RELEASE &&
         ctx.state.key_wait_key >= 16);

    if (invalid_configuration) {
        publish_result(ctx, CHIP8_STATUS_INVALID_CONFIGURATION, 0, 0, 0, 0,
                       results[lane]);
        return;
    }

    if (ctx.state.rng_state == 0) {
        ctx.state.rng_state = 1;
    }
    ctx.state.hires = ctx.state.hires != 0 ? 1 : 0;
    ctx.state.halted = ctx.state.halted != 0 ? 1 : 0;

    chip8_u64 cycles_executed = 0;
    chip8_u32 frames_executed = 0;
    chip8_u32 hashes_written = 0;
    chip8_u32 status = CHIP8_STATUS_TIMEOUT;

    if (config.run_mode == CHIP8_RUN_CYCLES) {
        for (chip8_u64 cycle = 0; cycle < config.max_cycles; ++cycle) {
            record_opcode_attempt(ctx);
            const StepStatus step_status = execute_step(ctx);
            ++cycles_executed;
            if (step_status == STEP_OK || step_status == STEP_WAITING_FOR_KEY) {
                continue;
            }
            status = public_status(step_status);
            break;
        }
    } else {
        for (chip8_u32 frame = 0; frame < config.max_frames; ++frame) {
            if (frame < config.input_frame_count) {
                const chip8_u64 input_index =
                    static_cast<chip8_u64>(lane) * config.input_stride + frame;
                ctx.state.keys = input_masks[input_index];
            }

            bool frame_completed = true;
            for (chip8_u32 cycle = 0; cycle < config.cycles_per_frame;
                 ++cycle) {
                record_opcode_attempt(ctx);
                const StepStatus step_status = execute_step(ctx);
                ++cycles_executed;
                if (step_status == STEP_OK ||
                    step_status == STEP_WAITING_FOR_KEY) {
                    continue;
                }
                status = public_status(step_status);
                frame_completed = false;
                break;
            }

            if (!frame_completed) {
                break;
            }

            if (ctx.state.dt > 0) {
                --ctx.state.dt;
            }
            if (ctx.state.st > 0) {
                --ctx.state.st;
            }

            const chip8_u64 hash = frame_hash(ctx);
            if (frames_executed < config.frame_hash_capacity) {
                const chip8_u64 hash_index =
                    static_cast<chip8_u64>(lane) * config.frame_hash_stride +
                    frames_executed;
                frame_hashes[hash_index] = hash;
                ++hashes_written;
            }
            ++frames_executed;
        }

        if (frames_executed == config.max_frames) {
            status = ctx.state.key_wait_kind != CHIP8_KEY_WAIT_NONE
                         ? CHIP8_STATUS_WAITING_FOR_INPUT
                         : CHIP8_STATUS_TIMEOUT;
        }
    }

    publish_result(ctx, status, cycles_executed, frames_executed,
                   hashes_written, frame_hash(ctx), results[lane]);
}
