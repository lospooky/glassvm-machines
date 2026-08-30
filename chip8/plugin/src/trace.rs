use std::collections::BTreeMap;

use glassvm_core::{
    AccessDetail, Address, CausalRelation, ControlFlow, ControlFlowKind, EventContext, EventKind,
    EventSelection, ExecutionEvent, InstructionRef, IoChannel, IoDirection, IoObservation,
    NativeEvent, ObservationRequest, SinkError, SnapshotCapture, StateLocation, StateRead,
    StateSpace, StateWrite, TrapInfo,
};
use serde_json::{Value, json};

use crate::identity::schema;

pub(super) fn observation_requires_step_records(plan: &ObservationRequest) -> bool {
    plan.normalized_events.state_diffs
        || matches!(plan.snapshots.capture, SnapshotCapture::EverySteps(_))
        || matches!(
            &plan.normalized_events.events,
            EventSelection::Effects | EventSelection::Instructions | EventSelection::All
        )
        || matches!(&plan.normalized_events.events, EventSelection::Kinds(kinds) if kinds.iter().any(|kind| {
            !matches!(
                kind,
                EventKind::RunStarted
                    | EventKind::RunHalted
                    | EventKind::RunCrashed
                    | EventKind::FrameCompleted
                    | EventKind::SnapshotCaptured
            )
        }))
}

pub(super) fn causal_relation_for(kind: &EventKind) -> CausalRelation {
    match kind {
        EventKind::BranchTaken
        | EventKind::Call
        | EventKind::Return
        | EventKind::Interrupt
        | EventKind::Trap => CausalRelation::Control,
        EventKind::InputSampled => CausalRelation::Input,
        _ => CausalRelation::Data,
    }
}

pub(super) fn chip8_instruction_ref(
    record: &chip8_core::StepTraceRecord,
) -> Option<InstructionRef> {
    let opcode = record.opcode?;
    if opcode == 0xF000 {
        let hi_address = record.pc.wrapping_add(2);
        let lo_address = record.pc.wrapping_add(3);
        let [hi, lo] = match (
            recorded_memory_byte(record, hi_address),
            recorded_memory_byte(record, lo_address),
        ) {
            (Some(hi), Some(lo)) => [hi, lo],
            _ => record.after.i.to_be_bytes(),
        };
        let operand = u16::from_be_bytes([hi, lo]);
        return Some(InstructionRef {
            encoding: "xochip.word32.be".into(),
            bytes: vec![0xF0, 0x00, hi, lo],
            decoded: Some(format!("LD I, 0x{operand:04X}")),
        });
    }

    Some(InstructionRef {
        encoding: "chip8.word16.be".into(),
        bytes: opcode.to_be_bytes().to_vec(),
        decoded: Some(decode_chip8_opcode(opcode)),
    })
}

pub(super) fn recorded_memory_byte(
    record: &chip8_core::StepTraceRecord,
    address: u16,
) -> Option<u8> {
    record.memory_reads.iter().find_map(|read| {
        read.bytes.iter().enumerate().find_map(|(offset, byte)| {
            let offset = u16::try_from(offset).ok()?;
            (read.address.wrapping_add(offset) == address).then_some(*byte)
        })
    })
}

pub(super) fn is_long_instruction_operand(
    record: &chip8_core::StepTraceRecord,
    address: u16,
) -> bool {
    record.opcode == Some(0xF000)
        && matches!(
            address,
            candidate
                if candidate == record.pc.wrapping_add(2)
                    || candidate == record.pc.wrapping_add(3)
        )
}

pub(super) fn waiting_input_sample_event(
    context: &EventContext,
    record: &chip8_core::StepTraceRecord,
    detail: AccessDetail,
) -> Option<ExecutionEvent> {
    if record.opcode.is_some()
        || matches!(record.before.key_wait, chip8_core::KeyWait::None)
        || record
            .events
            .iter()
            .any(|event| matches!(event, chip8_core::Event::KeyWaitResolved { .. }))
    {
        return None;
    }

    let mut event = ExecutionEvent::from_context(context, EventKind::InputSampled);
    event.reads.push(observed_read(
        key_mask_location(),
        json!(record.before.keys),
        detail,
    ));
    event.io.push(IoObservation {
        port: "keypad_in".into(),
        direction: IoDirection::Input,
        channel: IoChannel::Keypad,
        value: json!({
            "mask": record.before.keys,
            "wait_before": key_wait_value(record.before.key_wait),
            "wait_after": key_wait_value(record.after.key_wait),
        }),
    });
    Some(event)
}

pub(super) fn conditional_branch_event(
    context: &EventContext,
    record: &chip8_core::StepTraceRecord,
) -> Option<ExecutionEvent> {
    let opcode = record.opcode?;
    let conditional = matches!((opcode >> 12) & 0xF, 0x3 | 0x4)
        || matches!((opcode >> 12) & 0xF, 0x5 | 0x9) && opcode & 0xF == 0
        || opcode & 0xF0FF == 0xE09E
        || opcode & 0xF0FF == 0xE0A1;
    let fallthrough = record.pc.wrapping_add(2);
    if !conditional || record.after.pc == fallthrough {
        return None;
    }

    let mut event = ExecutionEvent::from_context(context, EventKind::BranchTaken);
    event.control_flow = Some(ControlFlow {
        kind: ControlFlowKind::Branch,
        from: Some(Address::new("program", record.pc as u64)),
        to: Some(Address::new("program", record.after.pc as u64)),
        taken: true,
    });
    Some(event)
}

pub(super) fn randomness_sample_event(
    context: &EventContext,
    record: &chip8_core::StepTraceRecord,
) -> Option<ExecutionEvent> {
    let opcode = record.opcode?;
    if opcode >> 12 != 0xC {
        return None;
    }
    let register = ((opcode >> 8) & 0xF) as usize;
    let mask = (opcode & 0xFF) as u8;
    let mut event = ExecutionEvent::from_context(
        context,
        EventKind::Extension("chip8.randomness_sampled".into()),
    );
    event.io.push(IoObservation {
        port: "rng_in".into(),
        direction: IoDirection::Input,
        channel: IoChannel::Randomness,
        value: json!({
            "mask": mask,
            "raw_sample": record.after.rng_state as u8,
            "masked_value": record.after.v[register],
            "target_register": format!("V{register:X}"),
        }),
    });
    Some(event)
}

pub(super) fn fatal_step_trap(
    context: &EventContext,
    record: &chip8_core::StepTraceRecord,
) -> Option<ExecutionEvent> {
    let (code, message, evidence) = match &record.result {
        chip8_core::StepResult::StackOverflow => (
            "chip8.stack_overflow",
            "CHIP-8 call stack overflow".to_string(),
            json!({"stack_pointer": record.before.sp}),
        ),
        chip8_core::StepResult::StackUnderflow => (
            "chip8.stack_underflow",
            "CHIP-8 return with an empty call stack".to_string(),
            json!({"stack_pointer": record.before.sp}),
        ),
        chip8_core::StepResult::MemoryFault(address) => (
            "chip8.memory_fault",
            format!("CHIP-8 memory access crosses the first invalid byte 0x{address:X}"),
            json!({"first_invalid_address": address}),
        ),
        _ => return None,
    };
    let mut event = ExecutionEvent::from_context(context, EventKind::Trap);
    event.trap = Some(TrapInfo {
        code: code.into(),
        message,
        fatal: true,
    });
    event.control_flow = Some(ControlFlow {
        kind: ControlFlowKind::Trap,
        from: Some(Address::new("program", record.pc as u64)),
        to: None,
        taken: true,
    });
    event.extensions.insert("chip8.trap".into(), evidence);
    Some(event)
}

pub(super) fn named_location(
    space: StateSpace,
    name: impl Into<String>,
    width_bits: u16,
) -> StateLocation {
    let mut location = StateLocation::named(space, name);
    location.width_bits = Some(width_bits);
    location
}

pub(super) fn register_location(index: usize) -> StateLocation {
    named_location(StateSpace::Register, format!("V{index:X}"), 8)
}

pub(super) fn index_location() -> StateLocation {
    named_location(StateSpace::Register, "I", 16)
}

pub(super) fn timer_location(name: &str) -> StateLocation {
    named_location(StateSpace::Timer, name, 8)
}

pub fn stack_pointer_location() -> StateLocation {
    named_location(StateSpace::Stack, "SP", 8)
}

pub(super) fn stack_entry_location(index: usize) -> StateLocation {
    named_location(StateSpace::Stack, format!("stack[{index}]"), 16)
}

pub fn key_mask_location() -> StateLocation {
    named_location(StateSpace::Input, "keypad_mask", 16)
}

pub fn key_wait_location() -> StateLocation {
    named_location(StateSpace::Extension("chip8.key_wait".into()), "state", 16)
}

pub(super) fn key_wait_value(value: chip8_core::KeyWait) -> Value {
    match value {
        chip8_core::KeyWait::None => json!({"phase": "none"}),
        chip8_core::KeyWait::WaitPress(register) => {
            json!({"phase": "wait_press", "register": register})
        }
        chip8_core::KeyWait::WaitRelease(register, key) => {
            json!({"phase": "wait_release", "register": register, "key": key})
        }
    }
}

pub(super) fn observed_write(
    location: StateLocation,
    before: Value,
    after: Value,
    detail: AccessDetail,
) -> StateWrite {
    StateWrite {
        location,
        before: matches!(detail, AccessDetail::BeforeAndAfter).then_some(before),
        after: (!matches!(detail, AccessDetail::None)).then_some(after),
    }
}

pub(super) fn observed_read(
    location: StateLocation,
    value: Value,
    detail: AccessDetail,
) -> StateRead {
    StateRead {
        location,
        value: (!matches!(detail, AccessDetail::None)).then_some(value),
    }
}

pub(super) fn state_delta_events(
    context: &EventContext,
    record: &chip8_core::StepTraceRecord,
    detail: AccessDetail,
) -> Vec<ExecutionEvent> {
    use chip8_core::{Event, Timer};

    let mut events = Vec::new();
    let mut registers = ExecutionEvent::from_context(context, EventKind::RegisterWrite);
    for index in 0..record.before.v.len() {
        if record.before.v[index] != record.after.v[index] {
            registers.writes.push(observed_write(
                register_location(index),
                json!(record.before.v[index]),
                json!(record.after.v[index]),
                detail,
            ));
        }
    }
    if record.before.i != record.after.i {
        registers.writes.push(observed_write(
            index_location(),
            json!(record.before.i),
            json!(record.after.i),
            detail,
        ));
    }
    if !registers.writes.is_empty() {
        events.push(registers);
    }

    let delay_is_native = record.events.iter().any(|event| {
        matches!(
            event,
            Event::TimerSet {
                timer: Timer::Delay,
                ..
            }
        )
    });
    let sound_is_native = record.events.iter().any(|event| {
        matches!(
            event,
            Event::TimerSet {
                timer: Timer::Sound,
                ..
            }
        )
    });
    let mut timers = ExecutionEvent::from_context(context, EventKind::TimerChanged);
    if record.before.dt != record.after.dt && !delay_is_native {
        timers.writes.push(observed_write(
            timer_location("delay"),
            json!(record.before.dt),
            json!(record.after.dt),
            detail,
        ));
    }
    if record.before.st != record.after.st && !sound_is_native {
        timers.writes.push(observed_write(
            timer_location("sound"),
            json!(record.before.st),
            json!(record.after.st),
            detail,
        ));
    }
    if !timers.writes.is_empty() {
        events.push(timers);
    }

    let stack_is_native = record
        .events
        .iter()
        .any(|event| matches!(event, Event::StackPush | Event::StackPop));
    if !stack_is_native {
        let mut stack = ExecutionEvent::from_context(context, EventKind::StackChanged);
        if record.before.sp != record.after.sp {
            stack.writes.push(observed_write(
                stack_pointer_location(),
                json!(record.before.sp),
                json!(record.after.sp),
                detail,
            ));
        }
        for index in 0..record.before.stack.len() {
            if record.before.stack[index] != record.after.stack[index] {
                stack.writes.push(observed_write(
                    stack_entry_location(index),
                    json!(record.before.stack[index]),
                    json!(record.after.stack[index]),
                    detail,
                ));
            }
        }
        if !stack.writes.is_empty() {
            events.push(stack);
        }
    }

    let display_mode_is_native = record
        .events
        .iter()
        .any(|event| matches!(event, Event::HiresEnabled | Event::LoresEnabled));
    let mut display = ExecutionEvent::from_context(context, EventKind::DisplayWrite);
    if record.before.display_hires != record.after.display_hires && !display_mode_is_native {
        display.writes.push(observed_write(
            named_location(StateSpace::Display, "hires", 1),
            json!(record.before.display_hires),
            json!(record.after.display_hires),
            detail,
        ));
    }
    if record.before.display_plane != record.after.display_plane {
        display.writes.push(observed_write(
            named_location(StateSpace::Display, "plane_mask", 2),
            json!(record.before.display_plane),
            json!(record.after.display_plane),
            detail,
        ));
    }
    if !display.writes.is_empty() {
        events.push(display);
    }

    let key_wait_is_native = record
        .events
        .iter()
        .any(|event| matches!(event, Event::KeyWaitEntered | Event::KeyWaitResolved { .. }));
    let mut internal = ExecutionEvent::from_context(
        context,
        EventKind::Extension("chip8.internal_state_changed".into()),
    );
    if record.before.rng_state != record.after.rng_state {
        internal.writes.push(observed_write(
            named_location(StateSpace::Randomness, "rng_state", 64),
            json!(record.before.rng_state),
            json!(record.after.rng_state),
            detail,
        ));
    }
    for index in 0..record.before.flags.len() {
        if record.before.flags[index] != record.after.flags[index] {
            internal.writes.push(observed_write(
                named_location(
                    StateSpace::Extension("chip8.flags".into()),
                    format!("F{index:X}"),
                    8,
                ),
                json!(record.before.flags[index]),
                json!(record.after.flags[index]),
                detail,
            ));
        }
    }
    if record.before.audio_buf != record.after.audio_buf {
        internal.writes.push(observed_write(
            named_location(StateSpace::Extension("chip8.audio".into()), "pattern", 128),
            json!(record.before.audio_buf),
            json!(record.after.audio_buf),
            detail,
        ));
    }
    if record.before.audio_pitch != record.after.audio_pitch {
        internal.writes.push(observed_write(
            named_location(StateSpace::Output, "audio_pitch", 8),
            json!(record.before.audio_pitch),
            json!(record.after.audio_pitch),
            detail,
        ));
    }
    if record.before.halted != record.after.halted {
        internal.writes.push(observed_write(
            named_location(StateSpace::Extension("chip8.execution".into()), "halted", 1),
            json!(record.before.halted),
            json!(record.after.halted),
            detail,
        ));
    }
    if record.before.key_wait != record.after.key_wait && !key_wait_is_native {
        internal.writes.push(observed_write(
            key_wait_location(),
            key_wait_value(record.before.key_wait),
            key_wait_value(record.after.key_wait),
            detail,
        ));
    }
    if !internal.writes.is_empty() {
        events.push(internal);
    }
    events
}

pub(super) fn enrich_normalized_event(
    event: &mut ExecutionEvent,
    native: &chip8_core::Event,
    record: &chip8_core::StepTraceRecord,
    detail: AccessDetail,
) {
    use chip8_core::{Event, Timer};

    match native {
        Event::TimerSet { timer, .. } => {
            let (name, before, after) = match timer {
                Timer::Delay => ("delay", record.before.dt, record.after.dt),
                Timer::Sound => ("sound", record.before.st, record.after.st),
            };
            event.writes.clear();
            event.writes.push(observed_write(
                timer_location(name),
                json!(before),
                json!(after),
                detail,
            ));
        }
        Event::MemoryWrite { addr, .. } => {
            if let Some(change) = record
                .memory_changes
                .iter()
                .find(|change| change.address == *addr)
            {
                event.writes.clear();
                for (offset, (before, after)) in change.before.iter().zip(&change.after).enumerate()
                {
                    let address = change.address.wrapping_add(offset as u16);
                    let location = StateLocation {
                        width_bits: Some(8),
                        ..StateLocation::addressed(
                            StateSpace::Memory,
                            Address::new("memory", address as u64),
                        )
                    };
                    event.writes.push(observed_write(
                        location,
                        json!(before),
                        json!(after),
                        detail,
                    ));
                }
            }
        }
        Event::StackPush | Event::StackPop => {
            event.writes.push(observed_write(
                stack_pointer_location(),
                json!(record.before.sp),
                json!(record.after.sp),
                detail,
            ));
            for index in 0..record.before.stack.len() {
                if record.before.stack[index] != record.after.stack[index] {
                    event.writes.push(observed_write(
                        stack_entry_location(index),
                        json!(record.before.stack[index]),
                        json!(record.after.stack[index]),
                        detail,
                    ));
                }
            }
        }
        Event::KeyWaitEntered => {
            event.writes.push(observed_write(
                key_wait_location(),
                key_wait_value(record.before.key_wait),
                key_wait_value(record.after.key_wait),
                detail,
            ));
        }
        Event::KeyWaitResolved { key } => {
            event.reads.push(observed_read(
                key_mask_location(),
                json!(record.before.keys),
                detail,
            ));
            event.writes.push(observed_write(
                key_wait_location(),
                key_wait_value(record.before.key_wait),
                key_wait_value(record.after.key_wait),
                detail,
            ));
            if let Some(io) = event.io.first_mut() {
                io.value = json!({
                    "key": key,
                    "mask": record.before.keys,
                    "wait_before": key_wait_value(record.before.key_wait),
                    "wait_after": key_wait_value(record.after.key_wait),
                });
            }
        }
        Event::HiresEnabled | Event::LoresEnabled => {
            event.writes.push(observed_write(
                named_location(StateSpace::Display, "hires", 1),
                json!(record.before.display_hires),
                json!(record.after.display_hires),
                detail,
            ));
        }
        _ => {}
    }
}

pub(super) fn insert_v_read(
    reads: &mut BTreeMap<StateLocation, Value>,
    state: &chip8_core::CompactState,
    index: usize,
) {
    reads.insert(register_location(index), json!(state.v[index]));
}

pub(super) fn insert_display_reads(
    reads: &mut BTreeMap<StateLocation, Value>,
    state: &chip8_core::CompactState,
) {
    reads.insert(
        named_location(StateSpace::Display, "hires", 1),
        json!(state.display_hires),
    );
    reads.insert(
        named_location(StateSpace::Display, "plane_mask", 2),
        json!(state.display_plane),
    );
}

pub(super) fn chip8_instruction_reads(
    record: &chip8_core::StepTraceRecord,
    quirks: &chip8_core::QuirksConfig,
    detail: AccessDetail,
) -> Vec<StateRead> {
    let Some(word) = record.opcode else {
        return Vec::new();
    };
    let top = ((word >> 12) & 0xF) as u8;
    let x = ((word >> 8) & 0xF) as usize;
    let y = ((word >> 4) & 0xF) as usize;
    let n = (word & 0xF) as u8;
    let mut reads = BTreeMap::<StateLocation, Value>::new();

    match (top, x, y, n) {
        (0x0, 0x0, 0xE, 0x0)
        | (0x0, 0x0, 0xC, _)
        | (0x0, 0x0, 0xD, _)
        | (0x0, 0x0, 0xF, 0xB | 0xC) => {
            insert_display_reads(&mut reads, &record.before);
        }
        (0x0, 0x0, 0xE, 0xE) => {
            reads.insert(stack_pointer_location(), json!(record.before.sp));
            if record.before.sp > 0 {
                let index = record.before.sp as usize - 1;
                reads.insert(
                    stack_entry_location(index),
                    json!(record.before.stack[index]),
                );
            }
        }
        (0x2, _, _, _) => {
            reads.insert(stack_pointer_location(), json!(record.before.sp));
        }
        (0x3 | 0x4, _, _, _) | (0x7, _, _, _) => {
            insert_v_read(&mut reads, &record.before, x);
        }
        (0x5, _, _, 0x0) | (0x9, _, _, 0x0) => {
            insert_v_read(&mut reads, &record.before, x);
            insert_v_read(&mut reads, &record.before, y);
        }
        (0x5, _, _, 0x2) => {
            reads.insert(index_location(), json!(record.before.i));
            let (first, last) = if x <= y { (x, y) } else { (y, x) };
            for index in first..=last {
                insert_v_read(&mut reads, &record.before, index);
            }
        }
        (0x5, _, _, 0x3) => {
            reads.insert(index_location(), json!(record.before.i));
        }
        (0x8, _, _, 0x0) => {
            insert_v_read(&mut reads, &record.before, y);
        }
        (0x8, _, _, 0x1 | 0x2 | 0x3 | 0x4 | 0x5 | 0x7) => {
            insert_v_read(&mut reads, &record.before, x);
            insert_v_read(&mut reads, &record.before, y);
        }
        (0x8, _, _, 0x6 | 0xE) => {
            insert_v_read(
                &mut reads,
                &record.before,
                if quirks.shift_vx_only { x } else { y },
            );
        }
        (0xB, _, _, _) => {
            insert_v_read(
                &mut reads,
                &record.before,
                if quirks.jump_offset_vx { x } else { 0 },
            );
        }
        (0xC, _, _, _) => {
            reads.insert(
                named_location(StateSpace::Randomness, "rng_state", 64),
                json!(record.before.rng_state),
            );
        }
        (0xD, _, _, _) => {
            insert_v_read(&mut reads, &record.before, x);
            insert_v_read(&mut reads, &record.before, y);
            reads.insert(index_location(), json!(record.before.i));
            insert_display_reads(&mut reads, &record.before);
        }
        (0xE, _, 0x9, 0xE) | (0xE, _, 0xA, 0x1) => {
            insert_v_read(&mut reads, &record.before, x);
            reads.insert(key_mask_location(), json!(record.before.keys));
        }
        (0xF, _, 0x0, 0x7) => {
            reads.insert(timer_location("delay"), json!(record.before.dt));
        }
        (0xF, _, 0x1, 0x5 | 0x8) | (0xF, _, 0x2, 0x9) | (0xF, _, 0x3, 0x0 | 0xA) => {
            insert_v_read(&mut reads, &record.before, x);
        }
        (0xF, _, 0x1, 0xE) | (0xF, _, 0x3, 0x3) => {
            reads.insert(index_location(), json!(record.before.i));
            insert_v_read(&mut reads, &record.before, x);
        }
        (0xF, _, 0x5, 0x5) => {
            reads.insert(index_location(), json!(record.before.i));
            for index in 0..=x {
                insert_v_read(&mut reads, &record.before, index);
            }
        }
        (0xF, _, 0x6, 0x5) | (0xF, 0x0, 0x0, 0x2) => {
            reads.insert(index_location(), json!(record.before.i));
        }
        (0xF, _, 0x7, 0x5) => {
            for index in 0..=x.min(15) {
                insert_v_read(&mut reads, &record.before, index);
            }
        }
        (0xF, _, 0x8, 0x5) => {
            for index in 0..=x.min(15) {
                reads.insert(
                    named_location(
                        StateSpace::Extension("chip8.flags".into()),
                        format!("F{index:X}"),
                        8,
                    ),
                    json!(record.before.flags[index]),
                );
            }
        }
        _ => {}
    }

    for memory in &record.memory_reads {
        for (offset, byte) in memory.bytes.iter().enumerate() {
            let address = memory.address.wrapping_add(offset as u16);
            if is_long_instruction_operand(record, address) {
                continue;
            }
            let location = StateLocation {
                width_bits: Some(8),
                ..StateLocation::addressed(
                    StateSpace::Memory,
                    Address::new("memory", address as u64),
                )
            };
            reads.insert(location, json!(byte));
        }
    }

    reads
        .into_iter()
        .map(|(location, value)| observed_read(location, value, detail))
        .collect()
}

pub(super) fn sink_error(error: SinkError) -> String {
    format!("emission sink rejected run data: {error}")
}

pub(super) fn termination_from_step_result(
    result: &chip8_core::StepResult,
) -> chip8_core::TerminationReason {
    match result {
        chip8_core::StepResult::Halted => chip8_core::TerminationReason::Completed,
        chip8_core::StepResult::InvalidOpcode(_) => chip8_core::TerminationReason::InvalidOpcode,
        chip8_core::StepResult::StackOverflow => chip8_core::TerminationReason::StackOverflow,
        chip8_core::StepResult::StackUnderflow => chip8_core::TerminationReason::StackUnderflow,
        chip8_core::StepResult::MemoryFault(_) => chip8_core::TerminationReason::MemoryFault,
        chip8_core::StepResult::WaitingForKey => chip8_core::TerminationReason::WaitingForInput,
        chip8_core::StepResult::Ok => chip8_core::TerminationReason::Timeout,
    }
}

pub(super) fn native_event_from_chip8(event: &chip8_core::Event) -> NativeEvent {
    use chip8_core::Event;

    let (kind, payload) = match event {
        Event::ClearScreen => ("clear_screen", json!({})),
        Event::Jump { from, to } => ("jump", json!({"from": from, "to": to})),
        Event::Call { from, to } => ("call", json!({"from": from, "to": to})),
        Event::Return { to } => ("return", json!({"to": to})),
        Event::Draw { x, y, n, collision } => (
            "draw",
            json!({"x": x, "y": y, "n": n, "collision": collision}),
        ),
        Event::KeyWaitEntered => ("key_wait_entered", json!({})),
        Event::KeyWaitResolved { key } => ("key_wait_resolved", json!({"key": key})),
        Event::TimerSet { timer, value } => (
            "timer_set",
            json!({
                "timer": match timer {
                    chip8_core::Timer::Delay => "delay",
                    chip8_core::Timer::Sound => "sound",
                },
                "value": value,
            }),
        ),
        Event::MemoryWrite { addr, len } => ("memory_write", json!({"addr": addr, "len": len})),
        Event::StackPush => ("stack_push", json!({})),
        Event::StackPop => ("stack_pop", json!({})),
        Event::ScrollDown { n } => ("scroll_down", json!({"rows": n})),
        Event::ScrollUp { n } => ("scroll_up", json!({"rows": n})),
        Event::ScrollRight => ("scroll_right", json!({"pixels": 4})),
        Event::ScrollLeft => ("scroll_left", json!({"pixels": 4})),
        Event::HiresEnabled => ("hires_enabled", json!({})),
        Event::LoresEnabled => ("lores_enabled", json!({})),
        Event::InvalidOpcode { pc, opcode } => {
            ("invalid_opcode", json!({"pc": pc, "opcode": opcode}))
        }
    };
    NativeEvent {
        schema: schema("chip8.event"),
        kind: kind.into(),
        payload,
    }
}

pub(super) fn native_event_from_step(
    event: &chip8_core::Event,
    record: &chip8_core::StepTraceRecord,
) -> NativeEvent {
    if let chip8_core::Event::MemoryWrite { addr, len } = event
        && let Some(change) = record
            .memory_changes
            .iter()
            .find(|change| change.address == *addr)
    {
        return NativeEvent {
            schema: schema("chip8.event"),
            kind: "memory_write".into(),
            payload: json!({
                "addr": addr,
                "len": len,
                "before": change.before,
                "after": change.after,
            }),
        };
    }
    native_event_from_chip8(event)
}

pub(super) fn decode_chip8_opcode(word: u16) -> String {
    let top = (word >> 12) & 0xF;
    let x = (word >> 8) & 0xF;
    let y = (word >> 4) & 0xF;
    let n = word & 0xF;
    let nn = word & 0xFF;
    let nnn = word & 0xFFF;
    match (top, x, y, n) {
        (0x0, 0x0, 0xE, 0x0) => "CLS".into(),
        (0x0, 0x0, 0xE, 0xE) => "RET".into(),
        (0x0, 0x0, 0xF, 0xD) => "EXIT".into(),
        (0x0, 0x0, 0xF, 0xE) => "LORES".into(),
        (0x0, 0x0, 0xF, 0xF) => "HIRES".into(),
        (0x0, 0x0, 0xF, 0xB) => "SCR".into(),
        (0x0, 0x0, 0xF, 0xC) => "SCL".into(),
        (0x0, 0x0, 0xC, _) => format!("SCD 0x{n:X}"),
        (0x0, 0x0, 0xD, _) => format!("SCU 0x{n:X}"),
        (0x0, _, _, _) => format!("SYS 0x{nnn:03X}"),
        (0x1, _, _, _) => format!("JP 0x{nnn:03X}"),
        (0x2, _, _, _) => format!("CALL 0x{nnn:03X}"),
        (0x3, _, _, _) => format!("SE V{x:X}, 0x{nn:02X}"),
        (0x4, _, _, _) => format!("SNE V{x:X}, 0x{nn:02X}"),
        (0x5, _, _, 0x0) => format!("SE V{x:X}, V{y:X}"),
        (0x5, _, _, 0x2) => format!("SAVE V{x:X}..V{y:X}"),
        (0x5, _, _, 0x3) => format!("LOAD V{x:X}..V{y:X}"),
        (0x6, _, _, _) => format!("LD V{x:X}, 0x{nn:02X}"),
        (0x7, _, _, _) => format!("ADD V{x:X}, 0x{nn:02X}"),
        (0x8, _, _, 0x0) => format!("LD V{x:X}, V{y:X}"),
        (0x8, _, _, 0x1) => format!("OR V{x:X}, V{y:X}"),
        (0x8, _, _, 0x2) => format!("AND V{x:X}, V{y:X}"),
        (0x8, _, _, 0x3) => format!("XOR V{x:X}, V{y:X}"),
        (0x8, _, _, 0x4) => format!("ADD V{x:X}, V{y:X}"),
        (0x8, _, _, 0x5) => format!("SUB V{x:X}, V{y:X}"),
        (0x8, _, _, 0x6) => format!("SHR V{x:X}, V{y:X}"),
        (0x8, _, _, 0x7) => format!("SUBN V{x:X}, V{y:X}"),
        (0x8, _, _, 0xE) => format!("SHL V{x:X}, V{y:X}"),
        (0x9, _, _, 0x0) => format!("SNE V{x:X}, V{y:X}"),
        (0xA, _, _, _) => format!("LD I, 0x{nnn:03X}"),
        (0xB, _, _, _) => format!("JP V0/V{x:X}, 0x{nnn:03X}"),
        (0xC, _, _, _) => format!("RND V{x:X}, 0x{nn:02X}"),
        (0xD, _, _, _) => format!("DRW V{x:X}, V{y:X}, 0x{n:X}"),
        (0xE, _, 0x9, 0xE) => format!("SKP V{x:X}"),
        (0xE, _, 0xA, 0x1) => format!("SKNP V{x:X}"),
        (0xF, 0x0, 0x0, 0x0) => "LD I, long".into(),
        (0xF, 0x0, 0x0, 0x2) => "AUDIO".into(),
        (0xF, _, 0x0, 0x1) => format!("PLANE 0x{x:X}"),
        (0xF, _, 0x0, 0x7) => format!("LD V{x:X}, DT"),
        (0xF, _, 0x0, 0xA) => format!("LD V{x:X}, K"),
        (0xF, _, 0x1, 0x5) => format!("LD DT, V{x:X}"),
        (0xF, _, 0x1, 0x8) => format!("LD ST, V{x:X}"),
        (0xF, _, 0x1, 0xE) => format!("ADD I, V{x:X}"),
        (0xF, _, 0x2, 0x9) => format!("LD F, V{x:X}"),
        (0xF, _, 0x3, 0x0) => format!("LD HF, V{x:X}"),
        (0xF, _, 0x3, 0x3) => format!("BCD V{x:X}"),
        (0xF, _, 0x3, 0xA) => format!("PITCH V{x:X}"),
        (0xF, _, 0x5, 0x5) => format!("LD [I], V{x:X}"),
        (0xF, _, 0x6, 0x5) => format!("LD V{x:X}, [I]"),
        (0xF, _, 0x7, 0x5) => format!("SAVEFLAGS V{x:X}"),
        (0xF, _, 0x8, 0x5) => format!("LOADFLAGS V{x:X}"),
        _ => format!("UNKNOWN 0x{word:04X}"),
    }
}

pub(super) fn termination_to_string(term: &chip8_core::TerminationReason) -> String {
    use chip8_core::TerminationReason::*;
    match term {
        Completed => "Completed",
        InvalidOpcode => "InvalidOpcode",
        StackUnderflow => "StackUnderflow",
        StackOverflow => "StackOverflow",
        Timeout => "Timeout",
        WaitingForInput => "WaitingForInput",
        MemoryFault => "MemoryFault",
    }
    .to_string()
}

// ── Versioned snapshot serialization ──────────────────────────────────────────
// serde derive does not support arrays larger than [T; 32], and Snapshot holds
// [u8; 65536] (mem) and [[u8; 8192]; 2] (display_buf). The machine bundle owns
// this strict, versioned little-endian codec. It deliberately rejects trailing
// bytes and non-canonical values so the same state has one replay fingerprint.
