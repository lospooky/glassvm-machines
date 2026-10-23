use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

use chip8_bundle::Chip8Plugin;
use glassvm_core::{
    BudgetedSink, CapabilityId, CapabilityRequest, EmissionBudgets, EventKind, EventSelection,
    ExecutionControls, ExecutionRequest, FrameCapture, InputSchedule, MachineBundle,
    MachineConfiguration, NullSink, ObservationRequest, OverflowPolicy, SinkError,
};
use glassvm_recorder::{
    FileRunSession, MachineExecutionOutcome, PublicationOutcome, RecorderFinalizationOutcome,
    RecorderLimits,
};
use pico8_bundle::Pico8Plugin;
use serde_json::{Value, json};
use tic80_bundle::Tic80Plugin;

struct Args {
    machine: String,
    mode: String,
    frame_limit: u64,
    repetition: u32,
    artifact: PathBuf,
    configuration: MachineConfiguration,
    event_kinds: Vec<EventKind>,
    capability_id: String,
    output_path: Option<PathBuf>,
}

fn usage() -> ! {
    eprintln!(
        "paper_memory_trial --machine ID --mode MODE --frame-limit N --repetition N --artifact PATH --configuration-json JSON --event-kinds-json JSON --capability ID [--output-path PATH]"
    );
    std::process::exit(2)
}

fn parse_args() -> Args {
    let mut values = std::collections::BTreeMap::new();
    let mut iter = std::env::args().skip(1);
    while let Some(key) = iter.next() {
        let Some(value) = iter.next() else { usage() };
        values.insert(key, value);
    }
    let get = |key: &str| values.get(key).cloned().unwrap_or_else(|| usage());
    let parse = |key: &str| get(key).parse().unwrap_or_else(|_| usage());
    let mode = get("--mode");
    if !matches!(
        mode.as_str(),
        "disabled" | "events_null" | "normalizer_null" | "recorder"
    ) {
        usage();
    }
    let configuration: MachineConfiguration =
        serde_json::from_str(&get("--configuration-json")).unwrap_or_else(|_| usage());
    let event_kinds: Vec<EventKind> =
        serde_json::from_str(&get("--event-kinds-json")).unwrap_or_else(|_| usage());
    Args {
        machine: get("--machine"),
        mode,
        frame_limit: parse("--frame-limit"),
        repetition: get("--repetition").parse().unwrap_or_else(|_| usage()),
        artifact: get("--artifact").into(),
        configuration,
        event_kinds,
        capability_id: get("--capability"),
        output_path: values.get("--output-path").map(PathBuf::from),
    }
}

fn request_observation(args: &Args) -> Result<ObservationRequest, String> {
    let mut observation = ObservationRequest::summary();
    if args.mode != "disabled" {
        let kinds = args.event_kinds.iter().cloned().collect();
        observation.normalized_events.events = EventSelection::Kinds(kinds);
    }
    if matches!(args.mode.as_str(), "normalizer_null" | "recorder") {
        observation.frames.capture = FrameCapture::Hashes;
        observation.capabilities = vec![CapabilityRequest::required(CapabilityId::new(
            args.capability_id.clone(),
        )?)];
    }
    observation.budgets = EmissionBudgets::default();
    observation.overflow = OverflowPolicy::Fail;
    Ok(observation)
}

fn machine_bundle(machine: &str) -> Result<Box<dyn MachineBundle>, String> {
    match machine {
        "chip8" => Ok(Box::new(Chip8Plugin::new())),
        "pico8" => Ok(Box::new(Pico8Plugin::new())),
        "tic80" => Ok(Box::new(Tic80Plugin::new())),
        _ => Err(format!("unknown machine {machine}")),
    }
}

fn peak_rss_bytes() -> Option<u64> {
    let mut status = String::new();
    fs::File::open("/proc/self/status")
        .ok()?
        .read_to_string(&mut status)
        .ok()?;
    let kib = status.lines().find_map(|line| {
        line.strip_prefix("VmHWM:")
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.parse::<u64>().ok())
    })?;
    kib.checked_mul(1024)
}

fn directory_bytes(path: &Path) -> io::Result<u64> {
    let mut total = 0_u64;
    let mut pending = vec![path.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

fn recorder_limits() -> RecorderLimits {
    RecorderLimits {
        max_block_logical_bytes: 1_048_576,
        max_segment_records: 256,
        max_segment_logical_bytes: 2_097_152,
        max_buffered_bytes_per_channel: 1_048_576,
    }
}

fn run() -> Result<Value, String> {
    let args = parse_args();
    let artifact = fs::read(&args.artifact).map_err(|error| error.to_string())?;
    let bundle = machine_bundle(&args.machine)?;
    let descriptor = bundle.descriptor();
    let observation = request_observation(&args)?;
    let run_id = format!(
        "paper-cost-{}-{}-r{}",
        args.machine, args.frame_limit, args.repetition
    );
    let mut request = ExecutionRequest::new(
        run_id,
        descriptor.id.clone(),
        artifact.clone(),
        args.configuration.clone(),
        InputSchedule::empty(),
        observation.clone(),
        ExecutionControls {
            frame_limit: Some(args.frame_limit),
            ..ExecutionControls::default()
        },
    )
    .map_err(|error| error.to_string())?;

    let prepare_start = Instant::now();
    let prepared_observation = bundle
        .prepare_observation(&observation)
        .map_err(|errors| errors.join("; "))?;
    request = request.with_prepared_observation_id(prepared_observation.identity);
    let prepared_run = bundle.prepare_run(&request)?;
    let prepare_seconds = prepare_start.elapsed().as_secs_f64();
    let session_start = Instant::now();
    let mut session = bundle
        .emulator()
        .create_execution_with_prepared_run(
            &artifact,
            request.clone(),
            prepared_run,
            prepared_observation,
        )
        .map_err(|error| error.to_string())?;
    let session_construction_seconds = session_start.elapsed().as_secs_f64();
    let execution_start = Instant::now();

    let (
        execution,
        evidence,
        recorder_receipt,
        published,
        run_directory_bytes,
        lifecycle_seconds,
        directory_measurement_seconds,
    ) = if args.mode == "recorder" {
        let output_path = args
            .output_path
            .as_ref()
            .ok_or_else(|| "recorder mode requires --output-path".to_owned())?;
        let result = FileRunSession::run(
            &mut *session,
            &request,
            output_path,
            descriptor.machine_version.clone(),
            recorder_limits(),
        )
        .map_err(|error| error.to_string())?;
        let lifecycle_seconds = execution_start.elapsed().as_secs_f64();
        let directory_measurement_start = Instant::now();
        let bytes = directory_bytes(output_path).map_err(|error| error.to_string())?;
        let directory_measurement_seconds =
            Some(directory_measurement_start.elapsed().as_secs_f64());
        let execution = match result.machine_execution {
            MachineExecutionOutcome::Completed(execution) => execution,
            MachineExecutionOutcome::Failed { message }
            | MachineExecutionOutcome::InterruptedByEvidenceFailure { message } => {
                return Err(format!("machine execution did not complete: {message}"));
            }
        };
        let recorder_receipt = match result.recorder_finalization {
            RecorderFinalizationOutcome::Finalized(receipt) => Some(receipt),
            outcome => {
                return Err(format!("recorder did not finalize: {outcome:?}"));
            }
        };
        let published = matches!(result.publication, PublicationOutcome::Published);
        if !published || result.published.is_none() {
            return Err("recorder finalized without an atomic publication handle".into());
        }
        (
            serde_json::to_value(execution).map_err(|error| error.to_string())?,
            serde_json::to_value(result.evidence_fulfillment).map_err(|error| error.to_string())?,
            Some(serde_json::to_value(recorder_receipt).map_err(|error| error.to_string())?),
            published,
            Some(bytes),
            lifecycle_seconds,
            directory_measurement_seconds,
        )
    } else {
        let mut sink = BudgetedSink::new(
            NullSink,
            request.observation.clone(),
            request.run_id.clone(),
            request.machine_id.clone(),
            descriptor.machine_version.clone(),
        )
        .map_err(|error: SinkError| error.to_string())?;
        let execution = session.execute(&mut sink)?;
        let evidence = sink.emit_receipt().map_err(|error| error.to_string())?;
        let lifecycle_seconds = execution_start.elapsed().as_secs_f64();
        (
            serde_json::to_value(execution).map_err(|error| error.to_string())?,
            serde_json::to_value(evidence).map_err(|error| error.to_string())?,
            None,
            false,
            None,
            lifecycle_seconds,
            None,
        )
    };
    let peak_rss = peak_rss_bytes();

    Ok(json!({
        "schema": "glassvm.paper_memory_trial",
        "schema_version": "1.1.0",
        "trial": {
            "machine_id": args.machine,
            "mode": args.mode,
            "frame_limit": args.frame_limit,
            "repetition": args.repetition,
        },
        "source": {
            "artifact_path": args.artifact,
            "artifact_sha256": glassvm_core::ContentDigest::sha256(&artifact).to_string(),
        },
        "request": {
            "configuration": args.configuration,
            "input_schedule": "empty",
            "execution_controls": request.execution_controls,
            "observation": observation,
        },
        "execution": execution,
        "evidence_receipt": evidence,
        "recorder_receipt": recorder_receipt,
        "publication": {
            "attempted": args.mode == "recorder",
            "published": published,
            "directory_bytes": run_directory_bytes,
        },
        "timing": {
            "preparation_seconds": prepare_seconds,
            "session_construction_seconds": session_construction_seconds,
            "execution_lifecycle_seconds": lifecycle_seconds,
            "directory_measurement_seconds": directory_measurement_seconds,
        },
        "memory": {
            "peak_rss_bytes": peak_rss,
            "peak_rss_source": "/proc/self/status VmHWM",
            "allocator_high_water_bytes": null,
        }
    }))
}

fn main() {
    match run() {
        Ok(result) => println!(
            "{}",
            serde_json::to_string(&result).expect("serialize trial result")
        ),
        Err(error) => {
            eprintln!("paper-memory trial failed: {error}");
            std::process::exit(1);
        }
    }
}
