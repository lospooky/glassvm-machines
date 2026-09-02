use glassvm_core::{
    ExecutionControls, ExecutionRequest, InputSchedule, MachineBundle, MachineConfiguration,
    ObservationRequest, PreparedObservation, PreparedRun, VersionStamp,
};
use glassvm_recorder::{FileRunSession, RecorderLimits};
use pyo3::prelude::*;
use serde::de::DeserializeOwned;
use tic80_plugin::Tic80Plugin;

fn decode_optional<T: DeserializeOwned>(raw: &str, field: &str) -> PyResult<Option<T>> {
    serde_json::from_str(raw).map_err(|error| {
        pyo3::exceptions::PyValueError::new_err(format!("{field} is invalid JSON: {error}"))
    })
}

fn recorder_limits() -> RecorderLimits {
    RecorderLimits {
        max_block_logical_bytes: 1024 * 1024,
        max_segment_records: 256,
        max_segment_logical_bytes: 2 * 1024 * 1024,
        max_buffered_bytes_per_channel: 1024 * 1024,
    }
}

#[pyclass]
struct PreparedBundleRun {
    artifact: Vec<u8>,
    request: ExecutionRequest,
    prepared_run: PreparedRun,
    prepared_observation: PreparedObservation,
}

#[pyfunction]
#[pyo3(signature = (artifact, run_id, configuration_json, input_schedule_json, execution_controls_json, observation_json))]
fn prepare_run(
    artifact: &[u8],
    run_id: &str,
    configuration_json: &str,
    input_schedule_json: &str,
    execution_controls_json: &str,
    observation_json: &str,
) -> PyResult<PreparedBundleRun> {
    let bundle = Tic80Plugin::new();
    let configuration =
        decode_optional::<MachineConfiguration>(configuration_json, "configuration")?
            .map_or_else(
                || MachineConfiguration::defaults(&bundle.emulator().config_schema()),
                Ok,
            )
            .map_err(pyo3::exceptions::PyValueError::new_err)?;
    let input_schedule = decode_optional::<InputSchedule>(input_schedule_json, "input_schedule")?
        .unwrap_or_else(InputSchedule::empty);
    let execution_controls =
        decode_optional::<ExecutionControls>(execution_controls_json, "execution_controls")?
            .unwrap_or_default();
    let observation = decode_optional::<ObservationRequest>(observation_json, "observation")?
        .unwrap_or_else(ObservationRequest::summary);
    let request = ExecutionRequest::new(
        run_id,
        bundle.descriptor().id.clone(),
        artifact.to_vec(),
        configuration,
        input_schedule,
        observation.clone(),
        execution_controls,
    )
    .map_err(pyo3::exceptions::PyValueError::new_err)?;
    let prepared_observation = bundle
        .prepare_observation(&observation)
        .map_err(|errors| pyo3::exceptions::PyValueError::new_err(errors.join("; ")))?;
    let request = request.with_prepared_observation_id(prepared_observation.identity);
    let prepared_run = bundle
        .prepare_run(&request)
        .map_err(pyo3::exceptions::PyValueError::new_err)?;
    Ok(PreparedBundleRun {
        artifact: artifact.to_vec(),
        request,
        prepared_run,
        prepared_observation,
    })
}

#[pyfunction]
#[pyo3(signature = (prepared, output_path))]
fn execute_prepared(
    py: Python<'_>,
    prepared: PyRef<'_, PreparedBundleRun>,
    output_path: &str,
) -> PyResult<Py<PyAny>> {
    let bundle = Tic80Plugin::new();
    let mut session = bundle
        .emulator()
        .create_execution_with_prepared_run(
            &prepared.artifact,
            prepared.request.clone(),
            prepared.prepared_run.clone(),
            prepared.prepared_observation.clone(),
        )
        .map_err(pyo3::exceptions::PyValueError::new_err)?;
    let result = FileRunSession::run(
        session.as_mut(),
        &prepared.request,
        output_path,
        VersionStamp::from("tic80-python-bundle"),
        recorder_limits(),
    )
    .map_err(|error| pyo3::exceptions::PyRuntimeError::new_err(error.to_string()))?;
    let encoded = serde_json::to_string(&serde_json::json!({
        "execution": result.execution,
        "evidence_receipt": result.evidence,
        "recorder_receipt": result.recorder,
        "published_run": {
            "path": output_path,
            "published": true
        }
    }))
    .map_err(|error| pyo3::exceptions::PyRuntimeError::new_err(error.to_string()))?;
    let result = py.import("json")?.call_method1("loads", (encoded,))?;
    if !result.is_instance_of::<pyo3::types::PyDict>() {
        return Err(pyo3::exceptions::PyRuntimeError::new_err(
            "internal run-result conversion did not produce a mapping",
        ));
    }
    Ok(result.unbind())
}

#[pyfunction]
fn bundle_provider() -> PyResult<String> {
    Ok(serde_json::json!({
        "protocol": "glassvm.python_bundle",
        "protocol_version": {"major": 1, "minor": 0, "patch": 0},
        "machine_id": "tic80",
        "distribution": "glassvm-machine-tic80",
        "module": "glassvm_py_tic80",
        "prepare_function": "prepare_run",
        "execute_function": "execute_prepared",
        "bundle_version": "0.1.0"
    })
    .to_string())
}

#[pymodule]
fn glassvm_py_tic80(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PreparedBundleRun>()?;
    m.add_function(wrap_pyfunction!(prepare_run, m)?)?;
    m.add_function(wrap_pyfunction!(execute_prepared, m)?)?;
    m.add_function(wrap_pyfunction!(bundle_provider, m)?)?;
    Ok(())
}
