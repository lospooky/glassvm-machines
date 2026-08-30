use cudarc::driver::DriverError;
use std::collections::TryReserveError;
use std::fmt;

/// Typed failure from an explicitly selected CUDA backend.
///
/// CUDA failures never cause an implicit CPU retry.
#[derive(Debug)]
pub enum CudaError {
    BackendUnavailable {
        detail: String,
    },
    InvalidDeviceOrdinal {
        requested: usize,
        available: usize,
    },
    UnsupportedRuntime {
        detail: String,
        source: Option<DriverError>,
    },
    UnsupportedConfiguration {
        detail: String,
    },
    KernelArtifact {
        detail: String,
    },
    BatchLimit {
        dimension: &'static str,
        requested: u128,
        maximum: u128,
    },
    SizeOverflow {
        allocation: &'static str,
    },
    HostAllocation {
        buffer: &'static str,
        elements: usize,
        source: TryReserveError,
    },
    InsufficientMemory {
        free_bytes: usize,
        reserve_bytes: usize,
        bytes_per_lane: usize,
    },
    DeviceQuery {
        operation: &'static str,
        source: DriverError,
    },
    ModuleLoad {
        source: DriverError,
    },
    SymbolLoad {
        symbol: &'static str,
        source: DriverError,
    },
    Allocation {
        buffer: &'static str,
        elements: usize,
        source: DriverError,
    },
    Copy {
        operation: &'static str,
        buffer: &'static str,
        source: DriverError,
    },
    Launch {
        source: DriverError,
    },
    Synchronization {
        source: DriverError,
    },
    DeviceReported {
        lane: usize,
        detail: String,
    },
}

impl fmt::Display for CudaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BackendUnavailable { detail } => write!(f, "CUDA backend unavailable: {detail}"),
            Self::InvalidDeviceOrdinal {
                requested,
                available,
            } => write!(
                f,
                "CUDA device ordinal {requested} is invalid; {available} device(s) are available"
            ),
            Self::UnsupportedRuntime { detail, .. } => {
                write!(f, "CUDA runtime/device is unsupported: {detail}")
            }
            Self::UnsupportedConfiguration { detail } => {
                write!(f, "CUDA configuration is unsupported: {detail}")
            }
            Self::KernelArtifact { detail } => {
                write!(f, "CUDA kernel artifact is invalid: {detail}")
            }
            Self::BatchLimit {
                dimension,
                requested,
                maximum,
            } => write!(
                f,
                "CUDA {dimension} {requested} exceeds the backend limit {maximum}"
            ),
            Self::SizeOverflow { allocation } => {
                write!(f, "checked size arithmetic overflowed for {allocation}")
            }
            Self::HostAllocation {
                buffer, elements, ..
            } => write!(
                f,
                "host allocation for {buffer} ({elements} elements) failed"
            ),
            Self::InsufficientMemory {
                free_bytes,
                reserve_bytes,
                bytes_per_lane,
            } => write!(
                f,
                "insufficient CUDA memory: {free_bytes} bytes free, {reserve_bytes} reserved, {bytes_per_lane} required per lane"
            ),
            Self::DeviceQuery { operation, source } => {
                write!(f, "CUDA device query {operation} failed: {source}")
            }
            Self::ModuleLoad { source } => write!(f, "CUDA PTX module load failed: {source}"),
            Self::SymbolLoad { symbol, source } => {
                write!(f, "CUDA kernel symbol {symbol} load failed: {source}")
            }
            Self::Allocation {
                buffer,
                elements,
                source,
            } => write!(
                f,
                "CUDA allocation for {buffer} ({elements} elements) failed: {source}"
            ),
            Self::Copy {
                operation,
                buffer,
                source,
            } => {
                write!(f, "CUDA {operation} copy for {buffer} failed: {source}")
            }
            Self::Launch { source } => write!(f, "CUDA kernel launch failed: {source}"),
            Self::Synchronization { source } => {
                write!(f, "CUDA synchronization failed: {source}")
            }
            Self::DeviceReported { lane, detail } => {
                write!(
                    f,
                    "CUDA lane {lane} reported an ABI/bounds failure: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for CudaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnsupportedRuntime {
                source: Some(source),
                ..
            }
            | Self::DeviceQuery { source, .. }
            | Self::ModuleLoad { source }
            | Self::SymbolLoad { source, .. }
            | Self::Allocation { source, .. }
            | Self::Copy { source, .. }
            | Self::Launch { source }
            | Self::Synchronization { source } => Some(source),
            Self::HostAllocation { source, .. } => Some(source),
            _ => None,
        }
    }
}
