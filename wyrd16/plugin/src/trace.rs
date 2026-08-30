//! Translation helpers for native transition evidence.

use glassvm_core::{AccessDetail, EventKind, ExecutionRequest};
use serde_json::{Value, json};
use wyrd16_core::NativeEffectKind;

pub(crate) fn event_kind(kind: NativeEffectKind) -> EventKind {
    match kind {
        NativeEffectKind::InstructionDecoded => EventKind::InstructionDecoded,
        NativeEffectKind::RunHalted => EventKind::RunHalted,
        NativeEffectKind::RegisterWrite => EventKind::RegisterWrite,
        NativeEffectKind::MemoryRead => EventKind::MemoryRead,
        NativeEffectKind::MemoryWrite => EventKind::MemoryWrite,
        NativeEffectKind::BranchTaken => EventKind::BranchTaken,
        NativeEffectKind::DisplayWrite => EventKind::DisplayWrite,
        NativeEffectKind::InputSampled => EventKind::InputSampled,
    }
}

pub(crate) fn access_before(request: &ExecutionRequest, value: u8) -> Option<Value> {
    match request.observation.normalized_events.access_detail {
        AccessDetail::BeforeAndAfter => Some(json!(value)),
        AccessDetail::None | AccessDetail::AfterOnly => None,
    }
}

pub(crate) fn access_after(request: &ExecutionRequest, value: u8) -> Option<Value> {
    match request.observation.normalized_events.access_detail {
        AccessDetail::None => None,
        AccessDetail::AfterOnly | AccessDetail::BeforeAndAfter => Some(json!(value)),
    }
}
