use glassvm_core::{
    ConfigField, ConfigFieldKind, EmulatorBackend, EmulatorSession, ExecutionRequest,
    MachineConfigSchema, MachineId, PreparedConfiguration, PreparedObservation, PreparedRun,
    StructuredValue,
};
use serde_json::json;

use crate::emulator_session::Tic80Session;
use crate::identity::{EMULATOR_VERSION, MACHINE_ID, schema};

pub struct Tic80EmulatorBackend;

impl EmulatorBackend for Tic80EmulatorBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(MACHINE_ID)
    }

    fn display_name(&self) -> String {
        "TIC-80 Lua compatibility runtime".into()
    }

    fn config_schema(&self) -> MachineConfigSchema {
        MachineConfigSchema {
            schema: schema("tic80.machine_configuration"),
            fields: vec![
                ConfigField {
                    key: "cycles_per_frame".into(),
                    label: "Cycles per frame".into(),
                    description: "One TIC callback boundary is one TIC-80 machine frame".into(),
                    kind: ConfigFieldKind::Int {
                        min: 1,
                        max: 1,
                        step: 1,
                    },
                    default_value: json!(1),
                },
                ConfigField {
                    key: "runtime".into(),
                    label: "Runtime".into(),
                    description: "Executable cartridge language".into(),
                    kind: ConfigFieldKind::Enum {
                        options: vec!["lua".into()],
                    },
                    default_value: json!("lua"),
                },
                ConfigField {
                    key: "machine_seed".into(),
                    label: "Machine seed".into(),
                    description: "Seed governing TIC-80 machine state evolution".into(),
                    kind: ConfigFieldKind::Int {
                        min: 0,
                        max: i64::MAX,
                        step: 1,
                    },
                    default_value: json!(0),
                },
            ],
        }
    }

    fn create_execution_with_prepared_run(
        &self,
        artifact: &[u8],
        request: ExecutionRequest,
        prepared_run: PreparedRun,
        prepared_observation: PreparedObservation,
    ) -> Result<Box<dyn EmulatorSession>, String> {
        request.validate_envelope()?;
        prepared_run.validate()?;
        prepared_observation.validate()?;
        if prepared_run.run_id != request.run_id {
            return Err("TIC-80 prepared run ID does not match the execution request".into());
        }
        if prepared_run.machine_id != request.machine_id
            || request.machine_id != MachineId::from(MACHINE_ID)
        {
            return Err("TIC-80 prepared run targets the wrong machine".into());
        }
        if request.prepared_observation_id != Some(prepared_observation.identity)
            || prepared_observation.request != request.observation
        {
            return Err("TIC-80 prepared observation does not match the execution request".into());
        }
        Tic80Session::new(artifact, request, prepared_run, prepared_observation)
            .map(|session| Box::new(session) as Box<dyn EmulatorSession>)
    }
}

pub(super) fn configuration_values(
    configuration: &PreparedConfiguration,
) -> Result<(u32, u64), String> {
    let values = match &configuration.value {
        StructuredValue::Map(values) => values,
        _ => return Err("TIC-80 machine configuration must be a map".into()),
    };
    let cycles = unsigned(values.get("cycles_per_frame"), "cycles_per_frame")?;
    if cycles != 1 {
        return Err("TIC-80 requires cycles_per_frame=1".into());
    }
    let runtime = match values.get("runtime") {
        Some(StructuredValue::Text(value)) => value,
        _ => return Err("TIC-80 configuration field \"runtime\" must be text".into()),
    };
    if runtime != "lua" {
        return Err(format!(
            "unsupported TIC-80 runtime {runtime:?}; expected \"lua\""
        ));
    }
    let seed = unsigned(values.get("machine_seed"), "machine_seed")?;
    Ok((
        u32::try_from(cycles).map_err(|_| "cycles_per_frame is too large")?,
        seed,
    ))
}

fn unsigned(value: Option<&StructuredValue>, field: &str) -> Result<u64, String> {
    match value {
        Some(StructuredValue::Unsigned(value)) => Ok(*value),
        Some(StructuredValue::Signed(value)) => u64::try_from(*value)
            .map_err(|_| format!("TIC-80 configuration field {field:?} must be non-negative")),
        _ => Err(format!(
            "TIC-80 configuration field {field:?} must be an integer"
        )),
    }
}

#[allow(dead_code)]
pub(super) fn implementation_identity() -> &'static str {
    EMULATOR_VERSION
}
