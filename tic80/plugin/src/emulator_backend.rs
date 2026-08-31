use glassvm_core::{
    ConfigField, ConfigFieldKind, EmulatorBackend, EmulatorSession, ExecutionRequest,
    MachineConfigSchema, MachineId,
};
use serde_json::json;
use tic80_core::{Tic80Runtime, validate_runtime_configuration};

use crate::emulator_session::Tic80Session;
use crate::identity::MACHINE_ID;
use crate::replay::{resolve_episode_context, validate_stimuli};
use crate::session_snapshot::SessionLifecycle;

pub struct Tic80EmulatorBackend;

impl EmulatorBackend for Tic80EmulatorBackend {
    fn machine_id(&self) -> MachineId {
        MachineId::from(MACHINE_ID)
    }

    fn display_name(&self) -> String {
        "TIC-80 (Lua 5.4 compatibility runtime)".into()
    }

    fn config_schema(&self) -> MachineConfigSchema {
        MachineConfigSchema {
            fields: vec![ConfigField {
                key: "runtime".into(),
                label: "Runtime".into(),
                description: "Executable cartridge language".into(),
                kind: ConfigFieldKind::Enum {
                    options: vec!["lua".into()],
                },
                default_value: json!("lua"),
            }],
        }
    }

    fn create_execution(
        &self,
        rom_bytes: &[u8],
        mut request: ExecutionRequest,
    ) -> Result<Box<dyn EmulatorSession>, String> {
        request.validate_envelope()?;
        if request.machine_id != MachineId::from(MACHINE_ID) {
            return Err(format!(
                "TIC-80 backend cannot execute machine {:?}",
                request.machine_id
            ));
        }
        request.observation.validate()?;
        validate_runtime_configuration(
            request.config.cycles_per_frame,
            &request.config.machine_params,
        )?;
        validate_stimuli(&request)?;
        let episode = resolve_episode_context(&request)?;
        request = request.with_resolved_episode(episode)?;
        let runtime = Tic80Runtime::new(rom_bytes, request.config.seed)?;
        Ok(Box::new(Tic80Session {
            request,
            runtime,
            input_mask: 0,
            input_override_pending: false,
            next_stimulus: 0,
            lifecycle: SessionLifecycle::Fresh,
        }))
    }
}
