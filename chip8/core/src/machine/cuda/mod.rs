//! Optional native CUDA batch backend.
//!
//! The runtime consumes a checked-in PTX artifact through CUDA's dynamically
//! loaded Driver API. It never invokes NVRTC and never silently falls back to
//! the CPU evaluator.

mod abi;
mod error;
#[cfg(test)]
mod parity_tests;

pub use error::CudaError;

use self::abi::*;
use crate::cpu::{Cpu, KeyWait, MEM_SIZE};
use crate::display::BUF_SIZE;
use crate::execution::{EvalConfig, effective_seed};
use crate::machine::randomness::Rng;
use crate::policy::RunPolicy;
use crate::snapshot::Snapshot;
use crate::summary::TerminationReason;
use crate::{MACHINE_ID, SEMANTICS};
use cudarc::driver::{
    CudaContext, CudaFunction, CudaSlice, CudaStream, LaunchConfig, PushKernelArg,
};
use cudarc::nvrtc::Ptx;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::mem::MaybeUninit;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

const BACKEND_ID: &str = "chip8.cuda.driver";
const BACKEND_CONTRACT_VERSION: u32 = 1;
const CUDARC_VERSION: &str = "0.19.8";
const EMBEDDED_PTX: &str = include_str!("kernel.ptx");
const KERNEL_SOURCE: &[u8] = include_bytes!("kernel.cu");
const DISPLAY_BYTES: usize = 2 * BUF_SIZE;
const HOST_TRANSPOSE_TILE: usize = 32;
const MEMORY_BUDGET_NUMERATOR: usize = 3;
const MEMORY_BUDGET_DENOMINATOR: usize = 4;
const DEFAULT_MEMORY_RESERVE_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_THREADS_PER_BLOCK: u32 = 256;

/// Caller-controlled limits for CUDA launch chunking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CudaBatchOptions {
    /// Hard cap on lanes in one launch. `None` leaves memory as the limit.
    pub max_chunk_lanes: Option<usize>,
    /// Free device memory kept outside the backend's 75% working budget.
    pub memory_reserve_bytes: usize,
    /// One-dimensional CUDA block size.
    pub threads_per_block: u32,
}

impl Default for CudaBatchOptions {
    fn default() -> Self {
        Self {
            max_chunk_lanes: None,
            memory_reserve_bytes: DEFAULT_MEMORY_RESERVE_BYTES,
            threads_per_block: DEFAULT_THREADS_PER_BLOCK,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CudaKernelIdentity {
    pub symbol: String,
    pub abi_version: u32,
    pub source_sha256: String,
    pub ptx_sha256: String,
    pub ptx_isa: String,
    pub virtual_architecture: String,
    pub cudarc_version: String,
}

/// Driver-JIT properties of the loaded kernel on the selected device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CudaKernelRuntimeProperties {
    pub registers_per_thread: i32,
    pub static_shared_memory_bytes: i32,
    pub local_memory_bytes_per_thread: i32,
    pub max_threads_per_block: i32,
    pub active_blocks_per_multiprocessor: u32,
    pub ptx_version: i32,
    pub binary_version: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CudaDeviceIdentity {
    pub ordinal: usize,
    pub uuid: Option<String>,
    pub name: String,
    pub compute_capability: (i32, i32),
    pub total_memory_bytes: usize,
    pub driver_version: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CudaLaunchIdentity {
    pub threads_per_block: u32,
    pub max_chunk_lanes: Option<usize>,
    pub memory_reserve_bytes: usize,
    pub memory_budget_numerator: usize,
    pub memory_budget_denominator: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CudaBackendIdentity {
    pub backend_id: String,
    pub backend_contract_version: u32,
    pub machine_id: String,
    pub semantics: String,
    pub core_version: String,
    pub kernel: CudaKernelIdentity,
    pub kernel_runtime: CudaKernelRuntimeProperties,
    pub device: CudaDeviceIdentity,
    pub launch: CudaLaunchIdentity,
}

/// Exact launch-accumulated counters produced by the CUDA interpreter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CudaExecutionCounters {
    pub draw_count: u32,
    pub collision_count: u32,
    /// Counts both FX0A entry and resolution, matching CPU event accounting.
    pub input_opcode_count: u32,
    pub clear_count: u32,
    pub delay_timer_set_count: u32,
    pub delay_timer_nonzero_count: u32,
    pub sound_timer_set_count: u32,
    pub sound_timer_nonzero_count: u32,
    pub scroll_count: u32,
    pub opcode_counts: [u32; 16],
}

impl CudaExecutionCounters {
    fn add_launch(&mut self, raw: &RawLaneResult) {
        self.draw_count = self.draw_count.saturating_add(raw.draw_count);
        self.collision_count = self.collision_count.saturating_add(raw.collision_count);
        self.input_opcode_count = self
            .input_opcode_count
            .saturating_add(raw.input_opcode_count);
        self.clear_count = self.clear_count.saturating_add(raw.clear_count);
        self.delay_timer_set_count = self
            .delay_timer_set_count
            .saturating_add(raw.delay_timer_set_count);
        self.delay_timer_nonzero_count = self
            .delay_timer_nonzero_count
            .saturating_add(raw.delay_timer_nonzero_count);
        self.sound_timer_set_count = self
            .sound_timer_set_count
            .saturating_add(raw.sound_timer_set_count);
        self.sound_timer_nonzero_count = self
            .sound_timer_nonzero_count
            .saturating_add(raw.sound_timer_nonzero_count);
        self.scroll_count = self.scroll_count.saturating_add(raw.scroll_count);
        for (total, delta) in self.opcode_counts.iter_mut().zip(raw.opcode_counts) {
            // CoverageTracker uses ordinary u32 addition, so resumed launches
            // preserve its release-build wrapping behavior.
            *total = total.wrapping_add(delta);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CudaFault {
    InvalidOpcode { opcode: u16 },
    Memory { address: u32 },
}

/// Exact result contract for an accelerated lane.
///
/// Rich CPU-only evidence (events, traces, topology, and interestingness) is
/// intentionally absent rather than synthesized.
#[derive(Clone)]
pub struct CudaRunResult {
    pub snapshot: Snapshot,
    pub requested_seed: u64,
    pub effective_seed: u64,
    pub rom_hash: u64,
    pub cycles: u64,
    pub frames: u64,
    pub termination: TerminationReason,
    pub counters: CudaExecutionCounters,
    pub frame_hashes: Option<Vec<u64>>,
    pub final_frame_hash: u64,
    pub fault: Option<CudaFault>,
    pub backend: CudaBackendIdentity,
}

/// Batch-first native CUDA evaluator bound to one device and kernel artifact.
pub struct CudaBatchEvaluator {
    context: Arc<CudaContext>,
    stream: Arc<CudaStream>,
    function: CudaFunction,
    options: CudaBatchOptions,
    identity: CudaBackendIdentity,
}

impl CudaBatchEvaluator {
    pub fn new(device_ordinal: usize) -> Result<Self, CudaError> {
        Self::with_options(device_ordinal, CudaBatchOptions::default())
    }

    pub fn with_options(
        device_ordinal: usize,
        options: CudaBatchOptions,
    ) -> Result<Self, CudaError> {
        validate_options(options)?;
        let kernel = embedded_kernel_identity()?;

        // SAFETY: this only asks cudarc's dynamic loader whether a driver
        // library can be opened; it does not initialize a device or retain a
        // raw handle.
        if !unsafe { cudarc::driver::sys::is_culib_present() } {
            return Err(CudaError::BackendUnavailable {
                detail: "the CUDA Driver library was not found".into(),
            });
        }

        let available = catch_driver_loader(CudaContext::device_count).map_err(|source| {
            CudaError::DeviceQuery {
                operation: "device_count",
                source,
            }
        })?;
        let available = usize::try_from(available.max(0)).unwrap_or(0);
        if device_ordinal >= available {
            return Err(CudaError::InvalidDeviceOrdinal {
                requested: device_ordinal,
                available,
            });
        }

        let context =
            catch_driver_loader(|| CudaContext::new(device_ordinal)).map_err(|source| {
                CudaError::DeviceQuery {
                    operation: "retain_primary_context",
                    source,
                }
            })?;
        let compute_capability =
            context
                .compute_capability()
                .map_err(|source| CudaError::DeviceQuery {
                    operation: "compute_capability",
                    source,
                })?;
        if compute_capability < (5, 2) {
            return Err(CudaError::UnsupportedRuntime {
                detail: format!(
                    "device compute capability {}.{} is below PTX target minimum 5.2",
                    compute_capability.0, compute_capability.1
                ),
                source: None,
            });
        }
        let max_threads = context
            .attribute(
                cudarc::driver::sys::CUdevice_attribute::CU_DEVICE_ATTRIBUTE_MAX_THREADS_PER_BLOCK,
            )
            .map_err(|source| CudaError::DeviceQuery {
                operation: "max_threads_per_block",
                source,
            })?;
        if options.threads_per_block > max_threads as u32 {
            return Err(CudaError::UnsupportedConfiguration {
                detail: format!(
                    "threads_per_block {} exceeds device limit {max_threads}",
                    options.threads_per_block
                ),
            });
        }

        let device = device_identity(&context, device_ordinal, compute_capability)?;
        let module = context
            .load_module(Ptx::from_src(EMBEDDED_PTX))
            .map_err(classify_module_error)?;
        let function =
            module
                .load_function(KERNEL_SYMBOL)
                .map_err(|source| CudaError::SymbolLoad {
                    symbol: KERNEL_SYMBOL,
                    source,
                })?;
        let kernel_runtime = kernel_runtime_properties(&function, options.threads_per_block)?;
        if options.threads_per_block > kernel_runtime.max_threads_per_block as u32 {
            return Err(CudaError::UnsupportedConfiguration {
                detail: format!(
                    "threads_per_block {} exceeds loaded kernel limit {}",
                    options.threads_per_block, kernel_runtime.max_threads_per_block
                ),
            });
        }
        let stream = context.default_stream();
        let identity = CudaBackendIdentity {
            backend_id: BACKEND_ID.into(),
            backend_contract_version: BACKEND_CONTRACT_VERSION,
            machine_id: MACHINE_ID.into(),
            semantics: SEMANTICS.into(),
            core_version: env!("CARGO_PKG_VERSION").into(),
            kernel,
            kernel_runtime,
            device,
            launch: CudaLaunchIdentity {
                threads_per_block: options.threads_per_block,
                max_chunk_lanes: options.max_chunk_lanes,
                memory_reserve_bytes: options.memory_reserve_bytes,
                memory_budget_numerator: MEMORY_BUDGET_NUMERATOR,
                memory_budget_denominator: MEMORY_BUDGET_DENOMINATOR,
            },
        };

        Ok(Self {
            context,
            stream,
            function,
            options,
            identity,
        })
    }

    pub fn identity(&self) -> &CudaBackendIdentity {
        &self.identity
    }

    /// Evaluate ROMs in stable caller order without a CPU fallback.
    ///
    /// ROM-load failures are lane-local strings, matching the established
    /// CPU batch API. Backend/configuration failures are typed outer errors.
    pub fn evaluate_batch(
        &self,
        roms: &[&[u8]],
        config: &EvalConfig,
    ) -> Result<Vec<Result<CudaRunResult, String>>, CudaError> {
        let plan = ExecutionPlan::from_config(config)?;
        if roms.is_empty() {
            return Ok(Vec::new());
        }

        let mut ordered: Vec<Option<Result<CudaRunResult, String>>> =
            std::iter::repeat_with(|| None).take(roms.len()).collect();
        let mut valid = Vec::with_capacity(roms.len());
        for (index, rom) in roms.iter().enumerate() {
            match PreparedLane::new(index, rom, config) {
                Ok(lane) => valid.push(lane),
                Err(error) => ordered[index] = Some(Err(error)),
            }
        }
        if valid.is_empty() {
            return Ok(ordered
                .into_iter()
                .map(|entry| entry.expect("every invalid lane has an outcome"))
                .collect());
        }

        if plan.is_zero_bound() {
            for lane in valid {
                let index = lane.original_index;
                ordered[index] = Some(Ok(self.zero_bound_result(lane, &plan)));
            }
        } else {
            let per_lane = plan.bytes_per_lane()?;
            let free = self
                .context
                .mem_get_info()
                .map_err(|source| CudaError::DeviceQuery {
                    operation: "free_memory",
                    source,
                })?
                .0;
            let chunk_lanes = chunk_lane_capacity(
                free,
                self.options.memory_reserve_bytes,
                per_lane,
                self.options.max_chunk_lanes,
                valid.len(),
            )?;
            for lanes in valid.chunks(chunk_lanes) {
                let results = self.execute_chunk(lanes, &plan)?;
                for (lane, result) in lanes.iter().zip(results) {
                    ordered[lane.original_index] = Some(Ok(result));
                }
            }
        }

        Ok(ordered
            .into_iter()
            .map(|entry| entry.expect("valid and invalid lanes preserve every slot"))
            .collect())
    }

    fn zero_bound_result(&self, lane: PreparedLane, plan: &ExecutionPlan) -> CudaRunResult {
        let final_frame_hash = hash_display(&lane.snapshot.display_buf);
        CudaRunResult {
            snapshot: lane.snapshot,
            requested_seed: lane.requested_seed,
            effective_seed: lane.effective_seed,
            rom_hash: lane.rom_hash,
            cycles: 0,
            frames: 0,
            termination: TerminationReason::Timeout,
            counters: CudaExecutionCounters::default(),
            frame_hashes: plan.record_frames().then(Vec::new),
            final_frame_hash,
            fault: None,
            backend: self.identity.clone(),
        }
    }

    fn execute_chunk(
        &self,
        lanes: &[PreparedLane],
        plan: &ExecutionPlan,
    ) -> Result<Vec<CudaRunResult>, CudaError> {
        let host = PackedChunk::new(lanes, plan)?;
        let mut device = DeviceChunk::new(&self.stream, host)?;
        match plan.kind {
            PlanKind::Stagnant {
                max_frames,
                window,
                threshold,
            } => self.execute_stagnant(lanes, &mut device, max_frames, window, threshold, plan),
            _ => {
                let config = plan.raw_config(lanes.len(), false)?;
                self.launch(&mut device, config)?;
                let raw_results = self.copy_results(&device)?;
                let buffers = self.copy_final_buffers(&device)?;
                self.finish_results(lanes, plan, raw_results, buffers)
            }
        }
    }

    fn execute_stagnant(
        &self,
        lanes: &[PreparedLane],
        device: &mut DeviceChunk,
        max_frames: u32,
        window: usize,
        threshold: f32,
        plan: &ExecutionPlan,
    ) -> Result<Vec<CudaRunResult>, CudaError> {
        let mut accumulators: Vec<LaneAccumulator> = lanes
            .iter()
            .map(|_| LaneAccumulator::new(plan.record_frames()))
            .collect();
        let mut next_states: Vec<RawLaneState> = lanes
            .iter()
            .map(|lane| RawLaneState::from_snapshot(&lane.snapshot))
            .collect();
        let mut remaining = lanes.len();

        for frame_index in 0..max_frames {
            self.copy_states_to_device(device, &next_states)?;
            let config = plan.raw_config(lanes.len(), true)?;
            self.launch(device, config)?;
            let raw_results = self.copy_results(device)?;
            let frame_hashes = self.copy_hashes(device)?;

            for lane_index in 0..lanes.len() {
                if accumulators[lane_index].finished {
                    continue;
                }
                let raw = raw_results[lane_index];
                validate_raw_result(lanes[lane_index].original_index, &raw, 1)?;
                accumulators[lane_index].absorb(&raw)?;

                let terminal = matches!(
                    raw.status,
                    STATUS_HALTED
                        | STATUS_INVALID_OPCODE
                        | STATUS_STACK_OVERFLOW
                        | STATUS_STACK_UNDERFLOW
                        | STATUS_MEMORY_FAULT
                );
                if terminal {
                    accumulators[lane_index].finish(raw);
                    remaining -= 1;
                    next_states[lane_index] = frozen_state(raw.final_state);
                    continue;
                }
                if raw.frames_executed != 1 || raw.frame_hashes_written != 1 {
                    return Err(CudaError::DeviceReported {
                        lane: lanes[lane_index].original_index,
                        detail: format!(
                            "resumable frame returned {} frames and {} hashes",
                            raw.frames_executed, raw.frame_hashes_written
                        ),
                    });
                }

                let hash = frame_hashes[lane_index];
                accumulators[lane_index].push_hash(hash, window);
                let stagnant = accumulators[lane_index].is_stagnant(window, threshold);
                let at_limit = frame_index + 1 == max_frames;
                if stagnant || at_limit {
                    accumulators[lane_index].finish(raw);
                    remaining -= 1;
                    next_states[lane_index] = frozen_state(raw.final_state);
                } else {
                    next_states[lane_index] = raw.final_state;
                }
            }
            if remaining == 0 {
                break;
            }
        }

        let buffers = self.copy_final_buffers(device)?;
        let mut raw_results = Vec::with_capacity(lanes.len());
        for (lane_index, accumulator) in accumulators.iter().enumerate() {
            raw_results.push(accumulator.as_raw(lanes[lane_index].original_index)?);
        }
        self.finish_accumulated_results(lanes, plan, &accumulators, raw_results, buffers)
    }

    /// The sole unsafe kernel-launch boundary. All slices are live through the
    /// following synchronization; their element counts were checked before
    /// allocation; RawConfig/State/Result match the C ABI; mutable buffers are
    /// disjoint kernel arguments with one writer per lane.
    fn launch(&self, device: &mut DeviceChunk, config: RawConfig) -> Result<(), CudaError> {
        let blocks = config.lane_count.div_ceil(self.options.threads_per_block);
        let launch = LaunchConfig {
            grid_dim: (blocks, 1, 1),
            block_dim: (self.options.threads_per_block, 1, 1),
            shared_mem_bytes: 0,
        };
        let mut args = self.stream.launch_builder(&self.function);
        args.arg(&config)
            .arg(&device.states)
            .arg(&mut device.memory)
            .arg(&mut device.display)
            .arg(&device.inputs)
            .arg(&mut device.hashes)
            .arg(&mut device.results);
        // SAFETY: invariants are documented on this function and encoded by
        // the fixed ABI plus checked DeviceChunk construction.
        unsafe { args.launch(launch) }.map_err(|source| CudaError::Launch { source })?;
        self.context
            .synchronize()
            .map_err(|source| CudaError::Synchronization { source })
    }

    fn copy_states_to_device(
        &self,
        device: &mut DeviceChunk,
        states: &[RawLaneState],
    ) -> Result<(), CudaError> {
        self.stream
            .memcpy_htod(states, &mut device.states)
            .map_err(|source| CudaError::Copy {
                operation: "host-to-device",
                buffer: "lane_states",
                source,
            })
    }

    fn copy_results(&self, device: &DeviceChunk) -> Result<Vec<RawLaneResult>, CudaError> {
        self.stream
            .clone_dtoh(&device.results)
            .map_err(|source| CudaError::Copy {
                operation: "device-to-host",
                buffer: "lane_results",
                source,
            })
    }

    fn copy_hashes(&self, device: &DeviceChunk) -> Result<Vec<u64>, CudaError> {
        self.stream
            .clone_dtoh(&device.hashes)
            .map_err(|source| CudaError::Copy {
                operation: "device-to-host",
                buffer: "frame_hashes",
                source,
            })
    }

    fn copy_final_buffers(&self, device: &DeviceChunk) -> Result<FinalBuffers, CudaError> {
        let memory = self
            .stream
            .clone_dtoh(&device.memory)
            .map_err(|source| CudaError::Copy {
                operation: "device-to-host",
                buffer: "memory",
                source,
            })?;
        let display =
            self.stream
                .clone_dtoh(&device.display)
                .map_err(|source| CudaError::Copy {
                    operation: "device-to-host",
                    buffer: "display",
                    source,
                })?;
        let hashes = self.copy_hashes(device)?;
        Ok(FinalBuffers {
            lane_count: device.lane_count,
            memory,
            display,
            hashes,
            hash_stride: device.hash_stride,
        })
    }

    fn finish_results(
        &self,
        lanes: &[PreparedLane],
        plan: &ExecutionPlan,
        raw_results: Vec<RawLaneResult>,
        buffers: FinalBuffers,
    ) -> Result<Vec<CudaRunResult>, CudaError> {
        let mut outputs = Vec::with_capacity(lanes.len());
        let lane_buffers = unpack_snapshot_buffers(&buffers)?;
        for (lane_index, ((lane, raw), lane_buffers)) in
            lanes.iter().zip(raw_results).zip(lane_buffers).enumerate()
        {
            validate_raw_result(lane.original_index, &raw, buffers.hash_stride)?;
            let frame_hashes = plan.record_frames().then(|| {
                buffers.hashes[lane_index * buffers.hash_stride
                    ..lane_index * buffers.hash_stride + raw.frame_hashes_written as usize]
                    .to_vec()
            });
            outputs.push(self.build_result(
                lane,
                raw,
                CudaExecutionCounters::from_raw(&raw),
                raw.cycles_executed,
                u64::from(raw.frames_executed),
                frame_hashes,
                lane_buffers,
                plan.scripted(),
            )?);
        }
        Ok(outputs)
    }

    fn finish_accumulated_results(
        &self,
        lanes: &[PreparedLane],
        _plan: &ExecutionPlan,
        accumulators: &[LaneAccumulator],
        raw_results: Vec<RawLaneResult>,
        buffers: FinalBuffers,
    ) -> Result<Vec<CudaRunResult>, CudaError> {
        let mut outputs = Vec::with_capacity(lanes.len());
        let lane_buffers = unpack_snapshot_buffers(&buffers)?;
        for (((lane, accumulator), raw), lane_buffers) in lanes
            .iter()
            .zip(accumulators)
            .zip(raw_results)
            .zip(lane_buffers)
        {
            outputs.push(self.build_result(
                lane,
                raw,
                accumulator.counters.clone(),
                accumulator.cycles,
                accumulator.frames,
                accumulator.recorded_hashes.clone(),
                lane_buffers,
                false,
            )?);
        }
        Ok(outputs)
    }

    #[allow(clippy::too_many_arguments)]
    fn build_result(
        &self,
        lane: &PreparedLane,
        raw: RawLaneResult,
        counters: CudaExecutionCounters,
        cycles: u64,
        frames: u64,
        frame_hashes: Option<Vec<u64>>,
        lane_buffers: LaneBuffers,
        scripted: bool,
    ) -> Result<CudaRunResult, CudaError> {
        let snapshot = snapshot_from_raw(raw.final_state, lane_buffers).map_err(|detail| {
            CudaError::DeviceReported {
                lane: lane.original_index,
                detail,
            }
        })?;
        let computed_hash = hash_display(&snapshot.display_buf);
        if computed_hash != raw.final_frame_hash {
            return Err(CudaError::DeviceReported {
                lane: lane.original_index,
                detail: format!(
                    "final display hash mismatch: device {:016x}, host {:016x}",
                    raw.final_frame_hash, computed_hash
                ),
            });
        }
        let (termination, fault) = termination_from_raw(raw.status, raw, scripted);
        Ok(CudaRunResult {
            snapshot,
            requested_seed: lane.requested_seed,
            effective_seed: lane.effective_seed,
            rom_hash: lane.rom_hash,
            cycles,
            frames,
            termination,
            counters,
            frame_hashes,
            final_frame_hash: raw.final_frame_hash,
            fault,
            backend: self.identity.clone(),
        })
    }
}

fn kernel_runtime_properties(
    function: &CudaFunction,
    threads_per_block: u32,
) -> Result<CudaKernelRuntimeProperties, CudaError> {
    let query = |operation, result: Result<i32, cudarc::driver::DriverError>| {
        result.map_err(|source| CudaError::DeviceQuery { operation, source })
    };
    Ok(CudaKernelRuntimeProperties {
        registers_per_thread: query("kernel_register_count", function.num_regs())?,
        static_shared_memory_bytes: query(
            "kernel_static_shared_memory",
            function.shared_size_bytes(),
        )?,
        local_memory_bytes_per_thread: query("kernel_local_memory", function.local_size_bytes())?,
        max_threads_per_block: query(
            "kernel_max_threads_per_block",
            function.max_threads_per_block(),
        )?,
        active_blocks_per_multiprocessor: function
            .occupancy_max_active_blocks_per_multiprocessor(threads_per_block, 0, None)
            .map_err(|source| CudaError::DeviceQuery {
                operation: "kernel_occupancy",
                source,
            })?,
        ptx_version: query("kernel_ptx_version", function.ptx_version())?,
        binary_version: query("kernel_binary_version", function.binary_version())?,
    })
}

impl CudaExecutionCounters {
    fn from_raw(raw: &RawLaneResult) -> Self {
        let mut counters = Self::default();
        counters.add_launch(raw);
        counters
    }
}

#[derive(Clone, Copy)]
enum PlanKind {
    Frames {
        max_frames: u32,
        scripted: bool,
    },
    Cycles {
        max_cycles: u64,
    },
    Stagnant {
        max_frames: u32,
        window: usize,
        threshold: f32,
    },
}

struct ExecutionPlan {
    kind: PlanKind,
    cycles_per_frame: u32,
    quirks: u32,
    record_frames: bool,
    input_masks: Option<Vec<u16>>,
}

impl ExecutionPlan {
    fn from_config(config: &EvalConfig) -> Result<Self, CudaError> {
        if config.record_events {
            return Err(CudaError::UnsupportedConfiguration {
                detail: "record_events requires the CPU/audited evidence path".into(),
            });
        }
        validate_input_script(config)?;
        let quirks = quirk_flags(config);
        let cycles_per_frame = config.cycles_per_frame.max(1);

        if let Some(script) = &config.input_script {
            // Preserve execution::evaluate's established behavior: attaching
            // any script selects framed input execution for every RunPolicy.
            let max_frames = match config.policy {
                RunPolicy::Frames(n) | RunPolicy::UntilHalt { max_frames: n } => n,
                RunPolicy::Cycles(n) => n / u64::from(cycles_per_frame),
                RunPolicy::UntilStagnant { max_frames, .. } => max_frames,
            };
            let max_frames = frame_bound(max_frames, "scripted frame bound")?;
            return Ok(Self {
                kind: PlanKind::Frames {
                    max_frames,
                    scripted: true,
                },
                cycles_per_frame,
                quirks,
                record_frames: config.record_frames,
                input_masks: Some(materialize_input_masks(script, max_frames)?),
            });
        }

        let kind = match config.policy {
            RunPolicy::Frames(max_frames) | RunPolicy::UntilHalt { max_frames } => {
                PlanKind::Frames {
                    max_frames: frame_bound(max_frames, "frame bound")?,
                    scripted: false,
                }
            }
            RunPolicy::Cycles(max_cycles) => PlanKind::Cycles { max_cycles },
            RunPolicy::UntilStagnant {
                max_frames,
                window,
                threshold,
            } => PlanKind::Stagnant {
                max_frames: frame_bound(max_frames, "stagnation frame bound")?,
                window: usize::try_from(window.max(1)).map_err(|_| {
                    CudaError::UnsupportedConfiguration {
                        detail: "stagnation window does not fit host usize".into(),
                    }
                })?,
                threshold,
            },
        };
        Ok(Self {
            kind,
            cycles_per_frame,
            quirks,
            record_frames: config.record_frames,
            input_masks: None,
        })
    }

    fn record_frames(&self) -> bool {
        self.record_frames
    }

    fn scripted(&self) -> bool {
        matches!(self.kind, PlanKind::Frames { scripted: true, .. })
    }

    fn is_zero_bound(&self) -> bool {
        match self.kind {
            PlanKind::Frames { max_frames, .. } | PlanKind::Stagnant { max_frames, .. } => {
                max_frames == 0
            }
            PlanKind::Cycles { max_cycles } => max_cycles == 0,
        }
    }

    fn hash_capacity(&self) -> usize {
        match self.kind {
            PlanKind::Frames { max_frames, .. } if self.record_frames => max_frames as usize,
            PlanKind::Stagnant { .. } => 1,
            _ => 0,
        }
    }

    fn bytes_per_lane(&self) -> Result<usize, CudaError> {
        let input_bytes = self.input_masks.as_ref().map_or(Ok(0), |masks| {
            checked_mul(masks.len(), size_of::<u16>(), "input masks")
        })?;
        let hash_bytes = checked_mul(
            self.hash_capacity().max(1),
            size_of::<u64>(),
            "frame hashes",
        )?;
        [
            MEM_SIZE,
            DISPLAY_BYTES,
            size_of::<RawLaneState>(),
            size_of::<RawLaneResult>(),
            input_bytes,
            hash_bytes,
        ]
        .into_iter()
        .try_fold(0usize, |total, part| {
            total.checked_add(part).ok_or(CudaError::SizeOverflow {
                allocation: "per-lane CUDA buffers",
            })
        })
    }

    fn raw_config(
        &self,
        lanes: usize,
        single_stagnation_frame: bool,
    ) -> Result<RawConfig, CudaError> {
        let lane_count = u32::try_from(lanes).map_err(|_| CudaError::BatchLimit {
            dimension: "chunk lane count",
            requested: lanes as u128,
            maximum: u128::from(u32::MAX),
        })?;
        let (run_mode, max_frames, max_cycles) = match self.kind {
            PlanKind::Frames { max_frames, .. } => (RUN_FRAMES, max_frames, 0),
            PlanKind::Cycles { max_cycles } => (RUN_CYCLES, 0, max_cycles),
            PlanKind::Stagnant { .. } if single_stagnation_frame => (RUN_FRAMES, 1, 0),
            PlanKind::Stagnant { .. } => unreachable!("stagnation uses single-frame launches"),
        };
        let input_count = self.input_masks.as_ref().map_or(0, Vec::len);
        let hash_capacity = self.hash_capacity();
        Ok(RawConfig {
            abi_version: ABI_VERSION,
            lane_count,
            run_mode,
            max_frames,
            max_cycles,
            cycles_per_frame: self.cycles_per_frame,
            input_frame_count: u32::try_from(input_count).map_err(|_| CudaError::SizeOverflow {
                allocation: "input frame count",
            })?,
            input_stride: u32::try_from(input_count).map_err(|_| CudaError::SizeOverflow {
                allocation: "input stride",
            })?,
            frame_hash_capacity: u32::try_from(hash_capacity).map_err(|_| {
                CudaError::SizeOverflow {
                    allocation: "frame hash capacity",
                }
            })?,
            frame_hash_stride: u32::try_from(hash_capacity).map_err(|_| {
                CudaError::SizeOverflow {
                    allocation: "frame hash stride",
                }
            })?,
            quirk_flags: self.quirks,
            reserved: 0,
            reserved_tail: 0,
        })
    }
}

struct PreparedLane {
    original_index: usize,
    requested_seed: u64,
    effective_seed: u64,
    rom_hash: u64,
    snapshot: Snapshot,
}

impl PreparedLane {
    fn new(index: usize, rom: &[u8], config: &EvalConfig) -> Result<Self, String> {
        let seed = effective_seed(rom, config.seed);
        let mut cpu = Cpu::new(config.quirks);
        cpu.rng = Rng::new(seed);
        cpu.load_rom(rom)?;
        let rom_hash = crate::emulator::fnv1a_64(rom);
        Ok(Self {
            original_index: index,
            requested_seed: config.seed,
            effective_seed: seed,
            rom_hash,
            snapshot: cpu.save_snapshot(),
        })
    }
}

struct PackedChunk {
    lane_count: usize,
    states: Vec<RawLaneState>,
    memory: Vec<u8>,
    display: Vec<u8>,
    inputs: Vec<u16>,
    hashes: Vec<u64>,
    results: Vec<RawLaneResult>,
    hash_stride: usize,
}

impl PackedChunk {
    fn new(lanes: &[PreparedLane], plan: &ExecutionPlan) -> Result<Self, CudaError> {
        let lane_count = lanes.len();
        let memory_len = checked_mul(MEM_SIZE, lane_count, "memory SoA")?;
        let display_len = checked_mul(DISPLAY_BYTES, lane_count, "display SoA")?;
        let mut memory = try_filled_vec(memory_len, 0, "memory SoA")?;
        let mut display = try_filled_vec(display_len, 0, "display SoA")?;
        let mut states = try_reserved_vec(lane_count, "lane states")?;
        states.extend(
            lanes
                .iter()
                .map(|lane| RawLaneState::from_snapshot(&lane.snapshot)),
        );
        pack_snapshot_buffers(lanes, &mut memory, &mut display);

        let inputs = if let Some(schedule) = &plan.input_masks {
            let len = checked_mul(schedule.len(), lane_count, "lane-major input masks")?;
            let mut values = try_reserved_vec(len, "input masks")?;
            for _ in 0..lane_count {
                values.extend_from_slice(schedule);
            }
            values
        } else {
            try_filled_vec(1, 0, "input masks")?
        };
        let hash_stride = plan.hash_capacity();
        let hashes = try_filled_vec(
            checked_mul(hash_stride.max(1), lane_count, "lane-major hashes")?,
            0,
            "frame hashes",
        )?;
        let results = try_filled_vec(lane_count, RawLaneResult::default(), "lane results")?;
        Ok(Self {
            lane_count,
            states,
            memory,
            display,
            inputs,
            hashes,
            results,
            hash_stride,
        })
    }
}

/// Transpose lane-major host snapshots into the address-major layout used by
/// the kernel. Small address tiles keep the active strided destination pages
/// bounded while each source snapshot is read contiguously.
fn pack_snapshot_buffers(lanes: &[PreparedLane], memory: &mut [u8], display: &mut [u8]) {
    let lane_count = lanes.len();
    debug_assert_eq!(memory.len(), MEM_SIZE * lane_count);
    debug_assert_eq!(display.len(), DISPLAY_BYTES * lane_count);

    for tile_start in (0..MEM_SIZE).step_by(HOST_TRANSPOSE_TILE) {
        let tile_end = (tile_start + HOST_TRANSPOSE_TILE).min(MEM_SIZE);
        for (lane_index, lane) in lanes.iter().enumerate() {
            for address in tile_start..tile_end {
                memory[address * lane_count + lane_index] = lane.snapshot.mem[address];
            }
        }
    }
    for tile_start in (0..DISPLAY_BYTES).step_by(HOST_TRANSPOSE_TILE) {
        let tile_end = (tile_start + HOST_TRANSPOSE_TILE).min(DISPLAY_BYTES);
        for (lane_index, lane) in lanes.iter().enumerate() {
            for plane_pixel in tile_start..tile_end {
                let plane = plane_pixel / BUF_SIZE;
                let pixel = plane_pixel % BUF_SIZE;
                display[plane_pixel * lane_count + lane_index] =
                    lane.snapshot.display_buf[plane][pixel];
            }
        }
    }
}

fn try_reserved_vec<T>(len: usize, buffer: &'static str) -> Result<Vec<T>, CudaError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|source| CudaError::HostAllocation {
            buffer,
            elements: len,
            source,
        })?;
    Ok(values)
}

fn try_filled_vec<T: Clone>(
    len: usize,
    value: T,
    buffer: &'static str,
) -> Result<Vec<T>, CudaError> {
    let mut values = try_reserved_vec(len, buffer)?;
    values.resize(len, value);
    Ok(values)
}

struct DeviceChunk {
    lane_count: usize,
    states: CudaSlice<RawLaneState>,
    memory: CudaSlice<u8>,
    display: CudaSlice<u8>,
    inputs: CudaSlice<u16>,
    hashes: CudaSlice<u64>,
    results: CudaSlice<RawLaneResult>,
    hash_stride: usize,
}

impl DeviceChunk {
    fn new(stream: &Arc<CudaStream>, host: PackedChunk) -> Result<Self, CudaError> {
        let states = copy_in(stream, &host.states, "lane_states")?;
        let memory = copy_in(stream, &host.memory, "memory")?;
        let display = copy_in(stream, &host.display, "display")?;
        let inputs = copy_in(stream, &host.inputs, "input_masks")?;
        let hashes = copy_in(stream, &host.hashes, "frame_hashes")?;
        let results = stream
            .alloc_zeros::<RawLaneResult>(host.results.len())
            .map_err(|source| CudaError::Allocation {
                buffer: "lane_results",
                elements: host.results.len(),
                source,
            })?;
        Ok(Self {
            lane_count: host.lane_count,
            states,
            memory,
            display,
            inputs,
            hashes,
            results,
            hash_stride: host.hash_stride,
        })
    }
}

fn copy_in<T: cudarc::driver::DeviceRepr>(
    stream: &Arc<CudaStream>,
    source: &[T],
    buffer: &'static str,
) -> Result<CudaSlice<T>, CudaError> {
    stream
        .clone_htod(source)
        .map_err(|source_error| CudaError::Allocation {
            buffer,
            elements: source.len(),
            source: source_error,
        })
}

struct FinalBuffers {
    lane_count: usize,
    memory: Vec<u8>,
    display: Vec<u8>,
    hashes: Vec<u64>,
    hash_stride: usize,
}

struct LaneBuffers {
    memory: Box<[u8; MEM_SIZE]>,
    display: Box<[[u8; BUF_SIZE]; 2]>,
}

/// Transpose address-major device output into final lane-owned snapshots.
/// The tile bounds active source pages, avoiding one full 64 KiB strided walk
/// per lane while preserving the kernel ABI byte for byte.
fn unpack_snapshot_buffers(buffers: &FinalBuffers) -> Result<Vec<LaneBuffers>, CudaError> {
    let expected_memory = checked_mul(MEM_SIZE, buffers.lane_count, "memory SoA")?;
    let expected_display = checked_mul(DISPLAY_BYTES, buffers.lane_count, "display SoA")?;
    if buffers.memory.len() != expected_memory || buffers.display.len() != expected_display {
        return Err(CudaError::DeviceReported {
            lane: 0,
            detail: format!(
                "final buffer dimensions are invalid: memory {} != {expected_memory}, display {} != {expected_display}",
                buffers.memory.len(),
                buffers.display.len(),
            ),
        });
    }

    let mut lanes = try_reserved_vec(buffers.lane_count, "unpacked snapshot buffers")?;
    for _ in 0..buffers.lane_count {
        lanes.push(LaneBuffers {
            memory: Box::new([0; MEM_SIZE]),
            display: Box::new([[0; BUF_SIZE]; 2]),
        });
    }

    for tile_start in (0..MEM_SIZE).step_by(HOST_TRANSPOSE_TILE) {
        let tile_end = (tile_start + HOST_TRANSPOSE_TILE).min(MEM_SIZE);
        for (lane_index, lane) in lanes.iter_mut().enumerate() {
            for address in tile_start..tile_end {
                lane.memory[address] = buffers.memory[address * buffers.lane_count + lane_index];
            }
        }
    }
    for tile_start in (0..DISPLAY_BYTES).step_by(HOST_TRANSPOSE_TILE) {
        let tile_end = (tile_start + HOST_TRANSPOSE_TILE).min(DISPLAY_BYTES);
        for (lane_index, lane) in lanes.iter_mut().enumerate() {
            for plane_pixel in tile_start..tile_end {
                let plane = plane_pixel / BUF_SIZE;
                let pixel = plane_pixel % BUF_SIZE;
                lane.display[plane][pixel] =
                    buffers.display[plane_pixel * buffers.lane_count + lane_index];
            }
        }
    }
    Ok(lanes)
}

struct LaneAccumulator {
    cycles: u64,
    frames: u64,
    counters: CudaExecutionCounters,
    recent_hashes: Vec<u64>,
    recorded_hashes: Option<Vec<u64>>,
    final_raw: Option<RawLaneResult>,
    finished: bool,
}

impl LaneAccumulator {
    fn new(record_frames: bool) -> Self {
        Self {
            cycles: 0,
            frames: 0,
            counters: CudaExecutionCounters::default(),
            recent_hashes: Vec::new(),
            recorded_hashes: record_frames.then(Vec::new),
            final_raw: None,
            finished: false,
        }
    }

    fn absorb(&mut self, raw: &RawLaneResult) -> Result<(), CudaError> {
        self.cycles =
            self.cycles
                .checked_add(raw.cycles_executed)
                .ok_or(CudaError::SizeOverflow {
                    allocation: "accumulated cycle count",
                })?;
        self.frames = self
            .frames
            .checked_add(u64::from(raw.frames_executed))
            .ok_or(CudaError::SizeOverflow {
                allocation: "accumulated frame count",
            })?;
        self.counters.add_launch(raw);
        Ok(())
    }

    fn push_hash(&mut self, hash: u64, window: usize) {
        if self.recent_hashes.len() == window {
            self.recent_hashes.remove(0);
        }
        self.recent_hashes.push(hash);
        if let Some(recorded) = &mut self.recorded_hashes {
            recorded.push(hash);
        }
    }

    fn is_stagnant(&self, window: usize, threshold: f32) -> bool {
        if self.recent_hashes.len() != window {
            return false;
        }
        let unique = self.recent_hashes.iter().collect::<HashSet<_>>().len();
        unique as f32 / (window as f32) < threshold
    }

    fn finish(&mut self, raw: RawLaneResult) {
        self.final_raw = Some(raw);
        self.finished = true;
    }

    fn as_raw(&self, lane: usize) -> Result<RawLaneResult, CudaError> {
        let mut raw = self.final_raw.ok_or_else(|| CudaError::DeviceReported {
            lane,
            detail: "resumable execution ended without a final result".into(),
        })?;
        raw.cycles_executed = self.cycles;
        raw.frames_executed = u32::try_from(self.frames).map_err(|_| CudaError::SizeOverflow {
            allocation: "resumed frame result",
        })?;
        raw.frame_hashes_written = self
            .recorded_hashes
            .as_ref()
            .map_or(0, |hashes| hashes.len() as u32);
        Ok(raw)
    }
}

impl RawLaneState {
    fn from_snapshot(snapshot: &Snapshot) -> Self {
        let (key_wait_kind, key_wait_reg, key_wait_key) = match snapshot.key_wait {
            KeyWait::None => (0, 0, 0),
            KeyWait::WaitPress(reg) => (1, reg, 0),
            KeyWait::WaitRelease(reg, key) => (2, reg, key),
        };
        Self {
            rng_state: snapshot.rng_state,
            i: snapshot.i,
            pc: snapshot.pc,
            stack: snapshot.stack,
            keys: snapshot.keys,
            v: snapshot.v,
            flags: snapshot.flags,
            audio_buf: snapshot.audio_buf,
            sp: snapshot.sp,
            dt: snapshot.dt,
            st: snapshot.st,
            hires: u8::from(snapshot.display_hires),
            plane: snapshot.display_plane,
            halted: u8::from(snapshot.halted),
            key_wait_kind,
            key_wait_reg,
            key_wait_key,
            audio_pitch: snapshot.audio_pitch,
        }
    }
}

fn snapshot_from_raw(raw: RawLaneState, buffers: LaneBuffers) -> Result<Snapshot, String> {
    if raw.sp > 16 {
        return Err(format!("invalid stack pointer {}", raw.sp));
    }
    if raw.hires > 1 || raw.halted > 1 {
        return Err("non-canonical boolean in final lane state".into());
    }
    let key_wait = match raw.key_wait_kind {
        0 => KeyWait::None,
        1 if raw.key_wait_reg < 16 => KeyWait::WaitPress(raw.key_wait_reg),
        2 if raw.key_wait_reg < 16 && raw.key_wait_key < 16 => {
            KeyWait::WaitRelease(raw.key_wait_reg, raw.key_wait_key)
        }
        kind => return Err(format!("invalid key-wait state {kind}")),
    };
    Ok(Snapshot {
        mem: buffers.memory,
        v: raw.v,
        i: raw.i,
        pc: raw.pc,
        stack: raw.stack,
        sp: raw.sp,
        dt: raw.dt,
        st: raw.st,
        display_buf: buffers.display,
        display_hires: raw.hires != 0,
        display_plane: raw.plane,
        keys: raw.keys,
        rng_state: if raw.rng_state == 0 { 1 } else { raw.rng_state },
        flags: raw.flags,
        audio_buf: raw.audio_buf,
        audio_pitch: raw.audio_pitch,
        halted: raw.halted != 0,
        key_wait,
    })
}

fn frozen_state(mut state: RawLaneState) -> RawLaneState {
    state.halted = 1;
    state
}

fn termination_from_raw(
    status: u32,
    raw: RawLaneResult,
    scripted: bool,
) -> (TerminationReason, Option<CudaFault>) {
    match status {
        STATUS_TIMEOUT => (TerminationReason::Timeout, None),
        STATUS_HALTED => (TerminationReason::Completed, None),
        STATUS_WAITING_FOR_INPUT if scripted => (TerminationReason::WaitingForInput, None),
        STATUS_WAITING_FOR_INPUT => (TerminationReason::Timeout, None),
        STATUS_INVALID_OPCODE => (
            TerminationReason::InvalidOpcode,
            Some(CudaFault::InvalidOpcode {
                opcode: raw.invalid_opcode,
            }),
        ),
        STATUS_STACK_OVERFLOW => (TerminationReason::StackOverflow, None),
        STATUS_STACK_UNDERFLOW => (TerminationReason::StackUnderflow, None),
        STATUS_MEMORY_FAULT => (
            TerminationReason::MemoryFault,
            Some(CudaFault::Memory {
                address: raw.fault_address,
            }),
        ),
        _ => unreachable!("validated status"),
    }
}

fn validate_raw_result(
    lane: usize,
    raw: &RawLaneResult,
    hash_capacity: usize,
) -> Result<(), CudaError> {
    if raw.abi_version != ABI_VERSION {
        return Err(CudaError::DeviceReported {
            lane,
            detail: format!("ABI version {} != {ABI_VERSION}", raw.abi_version),
        });
    }
    if raw.reserved != 0 || raw.reserved_counts != 0 {
        return Err(CudaError::DeviceReported {
            lane,
            detail: "non-zero reserved result field".into(),
        });
    }
    if raw.status == STATUS_INVALID_CONFIGURATION {
        return Err(CudaError::DeviceReported {
            lane,
            detail: "kernel rejected host configuration or initial state".into(),
        });
    }
    if raw.status > STATUS_INVALID_CONFIGURATION {
        return Err(CudaError::DeviceReported {
            lane,
            detail: format!("unknown status {}", raw.status),
        });
    }
    if raw.frame_hashes_written as usize > hash_capacity {
        return Err(CudaError::DeviceReported {
            lane,
            detail: format!(
                "kernel wrote {} hashes into capacity {hash_capacity}",
                raw.frame_hashes_written
            ),
        });
    }
    Ok(())
}

fn validate_options(options: CudaBatchOptions) -> Result<(), CudaError> {
    if options.max_chunk_lanes == Some(0) {
        return Err(CudaError::UnsupportedConfiguration {
            detail: "max_chunk_lanes must be positive when present".into(),
        });
    }
    if options.threads_per_block == 0 || options.threads_per_block > 1024 {
        return Err(CudaError::UnsupportedConfiguration {
            detail: "threads_per_block must be in 1..=1024".into(),
        });
    }
    Ok(())
}

fn validate_input_script(config: &EvalConfig) -> Result<(), CudaError> {
    if let Some(script) = &config.input_script
        && script.windows(2).any(|pair| pair[0].frame > pair[1].frame)
    {
        return Err(CudaError::UnsupportedConfiguration {
            detail: "input_script must be sorted by ascending frame".into(),
        });
    }
    Ok(())
}

fn materialize_input_masks(
    script: &[crate::input::InputEvent],
    max_frames: u32,
) -> Result<Vec<u16>, CudaError> {
    let mut masks = Vec::new();
    masks
        .try_reserve_exact(max_frames as usize)
        .map_err(|_| CudaError::SizeOverflow {
            allocation: "dense input schedule",
        })?;
    let mut current = 0u16;
    let mut event_index = 0usize;
    for frame in 0..u64::from(max_frames) {
        while event_index < script.len() && script[event_index].frame == frame {
            let event = script[event_index];
            if event.key < 16 {
                match event.action {
                    crate::input::KeyAction::Press => current |= 1 << event.key,
                    crate::input::KeyAction::Release => current &= !(1 << event.key),
                }
            }
            event_index += 1;
        }
        masks.push(current);
    }
    Ok(masks)
}

fn quirk_flags(config: &EvalConfig) -> u32 {
    u32::from(config.quirks.shift_vx_only)
        | (u32::from(config.quirks.load_store_no_inc_i) << 1)
        | (u32::from(config.quirks.display_wait) << 2)
        | (u32::from(config.quirks.clipping) << 3)
        | (u32::from(config.quirks.vf_reset_on_logic) << 4)
        | (u32::from(config.quirks.jump_offset_vx) << 5)
}

fn chunk_lane_capacity(
    free_bytes: usize,
    reserve_bytes: usize,
    bytes_per_lane: usize,
    caller_cap: Option<usize>,
    remaining_lanes: usize,
) -> Result<usize, CudaError> {
    let after_reserve = free_bytes.saturating_sub(reserve_bytes);
    let budget = checked_mul(after_reserve, MEMORY_BUDGET_NUMERATOR, "CUDA memory budget")?
        / MEMORY_BUDGET_DENOMINATOR;
    let by_memory = budget / bytes_per_lane;
    let capacity = by_memory
        .min(caller_cap.unwrap_or(usize::MAX))
        .min(remaining_lanes)
        .min(u32::MAX as usize);
    if capacity == 0 {
        return Err(CudaError::InsufficientMemory {
            free_bytes,
            reserve_bytes,
            bytes_per_lane,
        });
    }
    Ok(capacity)
}

fn checked_mul(left: usize, right: usize, allocation: &'static str) -> Result<usize, CudaError> {
    left.checked_mul(right)
        .ok_or(CudaError::SizeOverflow { allocation })
}

fn frame_bound(value: u64, dimension: &'static str) -> Result<u32, CudaError> {
    u32::try_from(value).map_err(|_| CudaError::BatchLimit {
        dimension,
        requested: u128::from(value),
        maximum: u128::from(u32::MAX),
    })
}

fn hash_display(display: &[[u8; BUF_SIZE]; 2]) -> u64 {
    let mut hash = 14695981039346656037u64;
    for byte in display[0].iter().chain(display[1].iter()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn ptx_directive(prefix: &str) -> Option<String> {
    EMBEDDED_PTX
        .lines()
        .find_map(|line| line.trim().strip_prefix(prefix).map(str::trim))
        .map(str::to_owned)
}

fn embedded_kernel_identity() -> Result<CudaKernelIdentity, CudaError> {
    let source_sha256 = sha256_hex(KERNEL_SOURCE);
    let declared_source =
        ptx_directive("// source-sha256:").ok_or_else(|| CudaError::KernelArtifact {
            detail: "PTX is missing source-sha256 metadata".into(),
        })?;
    if declared_source != source_sha256 {
        return Err(CudaError::KernelArtifact {
            detail: format!(
                "PTX source digest {declared_source} does not match kernel.cu {source_sha256}"
            ),
        });
    }
    if !EMBEDDED_PTX.contains(&format!(".entry {KERNEL_SYMBOL}(")) {
        return Err(CudaError::KernelArtifact {
            detail: format!("PTX does not export {KERNEL_SYMBOL}"),
        });
    }
    Ok(CudaKernelIdentity {
        symbol: KERNEL_SYMBOL.into(),
        abi_version: ABI_VERSION,
        source_sha256,
        ptx_sha256: sha256_hex(EMBEDDED_PTX.as_bytes()),
        ptx_isa: ptx_directive(".version").ok_or_else(|| CudaError::KernelArtifact {
            detail: "PTX is missing .version".into(),
        })?,
        virtual_architecture: ptx_directive("// virtual-architecture:").ok_or_else(|| {
            CudaError::KernelArtifact {
                detail: "PTX is missing virtual-architecture metadata".into(),
            }
        })?,
        cudarc_version: CUDARC_VERSION.into(),
    })
}

fn device_identity(
    context: &Arc<CudaContext>,
    ordinal: usize,
    compute_capability: (i32, i32),
) -> Result<CudaDeviceIdentity, CudaError> {
    let name = context.name().map_err(|source| CudaError::DeviceQuery {
        operation: "name",
        source,
    })?;
    let total_memory_bytes = context
        .total_mem()
        .map_err(|source| CudaError::DeviceQuery {
            operation: "total_memory",
            source,
        })?;
    let uuid = context.uuid().ok().map(|uuid| {
        uuid.bytes
            .iter()
            .map(|byte| format!("{:02x}", *byte as u8))
            .collect()
    });
    let mut driver_version = MaybeUninit::<i32>::uninit();
    // SAFETY: the CUDA driver is initialized by CudaContext::new and the
    // pointer refers to writable storage for exactly one C int.
    unsafe { cudarc::driver::sys::cuDriverGetVersion(driver_version.as_mut_ptr()) }
        .result()
        .map_err(|source| CudaError::DeviceQuery {
            operation: "driver_version",
            source,
        })?;
    // SAFETY: cuDriverGetVersion succeeded and therefore initialized it.
    let driver_version = unsafe { driver_version.assume_init() };
    Ok(CudaDeviceIdentity {
        ordinal,
        uuid,
        name,
        compute_capability,
        total_memory_bytes,
        driver_version,
    })
}

fn classify_module_error(source: cudarc::driver::DriverError) -> CudaError {
    use cudarc::driver::sys::CUresult;
    match source.0 {
        CUresult::CUDA_ERROR_NO_BINARY_FOR_GPU
        | CUresult::CUDA_ERROR_INVALID_PTX
        | CUresult::CUDA_ERROR_UNSUPPORTED_PTX_VERSION => CudaError::UnsupportedRuntime {
            detail: "driver cannot JIT the embedded PTX artifact".into(),
            source: Some(source),
        },
        _ => CudaError::ModuleLoad { source },
    }
}

fn catch_driver_loader<T>(
    operation: impl FnOnce() -> Result<T, cudarc::driver::DriverError>,
) -> Result<T, cudarc::driver::DriverError> {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(result) => result,
        Err(_) => Err(cudarc::driver::DriverError(
            cudarc::driver::sys::CUresult::CUDA_ERROR_NOT_INITIALIZED,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::QuirksConfig;
    use crate::input::{InputEvent, KeyAction};

    #[test]
    fn embedded_artifact_binds_source_symbol_and_abi() {
        let identity = embedded_kernel_identity().unwrap();
        assert_eq!(identity.abi_version, 1);
        assert_eq!(identity.symbol, "chip8_cuda_run_v1");
        assert_eq!(identity.source_sha256.len(), 64);
        assert_eq!(identity.ptx_sha256.len(), 64);
        assert_eq!(identity.virtual_architecture, "compute_52");
    }

    #[test]
    fn dense_input_masks_preserve_order_and_ignore_invalid_keys() {
        let script = vec![
            InputEvent {
                frame: 0,
                key: 2,
                action: KeyAction::Press,
            },
            InputEvent {
                frame: 0,
                key: 2,
                action: KeyAction::Release,
            },
            InputEvent {
                frame: 1,
                key: 15,
                action: KeyAction::Press,
            },
            InputEvent {
                frame: 2,
                key: 16,
                action: KeyAction::Press,
            },
        ];
        assert_eq!(
            materialize_input_masks(&script, 4).unwrap(),
            vec![0, 1 << 15, 1 << 15, 1 << 15]
        );
    }

    #[test]
    fn scripted_cycles_preserve_existing_framed_evaluate_semantics() {
        let mut config = EvalConfig::new(RunPolicy::Cycles(25));
        config.cycles_per_frame = 12;
        config.input_script = Some(Vec::new());
        let plan = ExecutionPlan::from_config(&config).unwrap();
        assert!(matches!(
            plan.kind,
            PlanKind::Frames {
                max_frames: 2,
                scripted: true
            }
        ));
    }

    #[test]
    fn unscripted_cycles_use_cycle_mode() {
        let config = EvalConfig::new(RunPolicy::Cycles(25));
        let plan = ExecutionPlan::from_config(&config).unwrap();
        assert!(matches!(plan.kind, PlanKind::Cycles { max_cycles: 25 }));
        assert_eq!(plan.hash_capacity(), 0);
        assert_eq!(
            plan.bytes_per_lane().unwrap(),
            MEM_SIZE
                + DISPLAY_BYTES
                + size_of::<RawLaneState>()
                + size_of::<RawLaneResult>()
                + size_of::<u64>()
        );
    }

    #[test]
    fn fx0a_bound_mapping_distinguishes_scripted_and_plain_frames() {
        let rom = [0xF0, 0x0A, 0x12, 0x00];
        let plain = EvalConfig::new(RunPolicy::Frames(1)).with_cycles_per_frame(1);
        let scripted = plain.clone().with_input_script(Vec::new());
        assert_eq!(
            crate::evaluate(&rom, &plain).unwrap().summary.termination,
            TerminationReason::Timeout
        );
        assert_eq!(
            crate::evaluate(&rom, &scripted)
                .unwrap()
                .summary
                .termination,
            TerminationReason::WaitingForInput
        );

        let raw = RawLaneResult {
            abi_version: ABI_VERSION,
            status: STATUS_WAITING_FOR_INPUT,
            ..RawLaneResult::default()
        };
        assert_eq!(
            termination_from_raw(raw.status, raw, false).0,
            TerminationReason::Timeout
        );
        assert_eq!(
            termination_from_raw(raw.status, raw, true).0,
            TerminationReason::WaitingForInput
        );
    }

    #[test]
    fn zero_cycles_per_frame_uses_cpu_effective_one_for_all_policies() {
        let mut frames = EvalConfig::new(RunPolicy::Frames(2));
        frames.cycles_per_frame = 0;
        assert_eq!(
            ExecutionPlan::from_config(&frames)
                .unwrap()
                .cycles_per_frame,
            1
        );

        let mut scripted_cycles = EvalConfig::new(RunPolicy::Cycles(2));
        scripted_cycles.cycles_per_frame = 0;
        scripted_cycles.input_script = Some(Vec::new());
        let plan = ExecutionPlan::from_config(&scripted_cycles).unwrap();
        assert_eq!(plan.cycles_per_frame, 1);
        assert!(matches!(
            plan.kind,
            PlanKind::Frames {
                max_frames: 2,
                scripted: true
            }
        ));
    }

    #[test]
    fn unsupported_rich_evidence_is_rejected() {
        let config = EvalConfig {
            record_events: true,
            ..EvalConfig::default()
        };
        assert!(matches!(
            ExecutionPlan::from_config(&config),
            Err(CudaError::UnsupportedConfiguration { .. })
        ));
    }

    #[test]
    fn frame_bounds_above_the_v1_abi_limit_are_typed() {
        let config = EvalConfig::new(RunPolicy::Frames(u64::from(u32::MAX) + 1));
        assert!(matches!(
            ExecutionPlan::from_config(&config),
            Err(CudaError::BatchLimit {
                dimension: "frame bound",
                ..
            })
        ));
    }

    #[test]
    fn rom_load_failures_remain_lane_local_inputs() {
        let config = EvalConfig::default();
        assert_eq!(
            PreparedLane::new(7, &[], &config).err().unwrap(),
            "ROM is empty"
        );
        let oversized = vec![0; crate::cpu::MAX_ROM_SIZE + 1];
        let error = PreparedLane::new(9, &oversized, &config).err().unwrap();
        assert!(error.starts_with("ROM too large:"));
    }

    #[test]
    fn device_result_rejects_every_reserved_field() {
        let raw = RawLaneResult {
            abi_version: ABI_VERSION,
            reserved_counts: 1,
            ..RawLaneResult::default()
        };
        assert!(matches!(
            validate_raw_result(4, &raw, 0),
            Err(CudaError::DeviceReported { lane: 4, .. })
        ));
    }

    #[test]
    fn chunk_capacity_honors_memory_reserve_fraction_and_caller_cap() {
        assert_eq!(
            chunk_lane_capacity(1_000, 200, 100, Some(4), 20).unwrap(),
            4
        );
        assert_eq!(chunk_lane_capacity(1_000, 200, 100, None, 20).unwrap(), 6);
        assert!(matches!(
            chunk_lane_capacity(100, 100, 1, None, 1),
            Err(CudaError::InsufficientMemory { .. })
        ));
    }

    #[test]
    fn snapshot_scalar_and_soa_round_trip_is_canonical() {
        let mut cpu = Cpu::new(QuirksConfig::schip());
        cpu.v[3] = 0xA5;
        cpu.i = 0x1234;
        cpu.pc = 0x4567;
        cpu.display.buf[1][77] = 1;
        cpu.display.hires = true;
        cpu.display.plane = 3;
        cpu.keys.from_mask(0x8010);
        cpu.key_wait = KeyWait::WaitRelease(3, 15);
        cpu.rng = Rng::from_state(42);
        let original = cpu.save_snapshot();
        let lane = PreparedLane {
            original_index: 0,
            requested_seed: 0,
            effective_seed: 0,
            rom_hash: 0,
            snapshot: original.clone(),
        };
        let plan = ExecutionPlan {
            kind: PlanKind::Frames {
                max_frames: 1,
                scripted: false,
            },
            cycles_per_frame: 1,
            quirks: 0,
            record_frames: false,
            input_masks: None,
        };
        let packed = PackedChunk::new(&[lane], &plan).unwrap();
        let raw = packed.states[0];
        let buffers = FinalBuffers {
            lane_count: packed.lane_count,
            memory: packed.memory,
            display: packed.display,
            hashes: packed.hashes,
            hash_stride: packed.hash_stride,
        };
        let lane_buffers = unpack_snapshot_buffers(&buffers).unwrap().pop().unwrap();
        let restored = snapshot_from_raw(raw, lane_buffers).unwrap();
        assert_eq!(restored.mem.as_ref(), original.mem.as_ref());
        assert_eq!(restored.display_buf.as_ref(), original.display_buf.as_ref());
        assert_eq!(restored.v, original.v);
        assert_eq!(restored.i, original.i);
        assert_eq!(restored.pc, original.pc);
        assert_eq!(restored.keys, original.keys);
        assert_eq!(restored.key_wait, original.key_wait);
        assert_eq!(restored.rng_state, original.rng_state);
    }

    #[test]
    fn tiled_snapshot_transpose_round_trips_multiple_lanes() {
        const LANE_COUNT: usize = 37;
        let mut originals = Vec::with_capacity(LANE_COUNT);
        let mut prepared = Vec::with_capacity(LANE_COUNT);
        for lane_index in 0..LANE_COUNT {
            let mut snapshot = Cpu::new(QuirksConfig::default()).save_snapshot();
            for (address, byte) in snapshot.mem.iter_mut().enumerate() {
                *byte = (address as u8)
                    .wrapping_mul(17)
                    .wrapping_add(lane_index as u8);
            }
            for plane in 0..2 {
                for (pixel, byte) in snapshot.display_buf[plane].iter_mut().enumerate() {
                    *byte = u8::from((pixel + 3 * plane + lane_index) % 11 == 0);
                }
            }
            originals.push(snapshot.clone());
            prepared.push(PreparedLane {
                original_index: lane_index,
                requested_seed: lane_index as u64,
                effective_seed: lane_index as u64 + 1,
                rom_hash: lane_index as u64 + 2,
                snapshot,
            });
        }
        let plan = ExecutionPlan {
            kind: PlanKind::Frames {
                max_frames: 1,
                scripted: false,
            },
            cycles_per_frame: 1,
            quirks: 0,
            record_frames: false,
            input_masks: None,
        };

        let packed = PackedChunk::new(&prepared, &plan).unwrap();
        for &address in &[0, 31, 32, 4095, 4096, MEM_SIZE - 1] {
            for (lane_index, original) in originals.iter().enumerate() {
                assert_eq!(
                    packed.memory[address * LANE_COUNT + lane_index],
                    original.mem[address],
                );
            }
        }
        let buffers = FinalBuffers {
            lane_count: packed.lane_count,
            memory: packed.memory,
            display: packed.display,
            hashes: packed.hashes,
            hash_stride: packed.hash_stride,
        };
        let restored = unpack_snapshot_buffers(&buffers).unwrap();
        for (actual, original) in restored.iter().zip(&originals) {
            assert_eq!(actual.memory, original.mem);
            assert_eq!(actual.display, original.display_buf);
        }
    }
}
