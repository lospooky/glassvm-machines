use glassvm_core::{
    CapabilityRequest, EmulatorBackend, MachineBundle, MachineContract, MachineDescriptor,
    Normalizer, NormalizerCatalog,
};

use crate::contract::contract;
use crate::descriptor::descriptor;
use crate::emulator_backend::Tic80EmulatorBackend;

pub struct Tic80Plugin {
    descriptor: MachineDescriptor,
    contract: MachineContract,
    emulator: Tic80EmulatorBackend,
}

impl Tic80Plugin {
    pub fn new() -> Self {
        Self {
            descriptor: descriptor(),
            contract: contract(),
            emulator: Tic80EmulatorBackend,
        }
    }
}

impl Default for Tic80Plugin {
    fn default() -> Self {
        Self::new()
    }
}

impl MachineBundle for Tic80Plugin {
    fn descriptor(&self) -> &MachineDescriptor {
        &self.descriptor
    }

    fn contract(&self) -> &MachineContract {
        &self.contract
    }

    fn emulator(&self) -> &dyn glassvm_core::EmulatorBackend {
        &self.emulator
    }

    fn static_analyzer(&self) -> Option<&dyn glassvm_core::StaticAnalyzerBackend> {
        None
    }

    fn verifier(&self) -> Option<&dyn glassvm_core::VerifierBackend> {
        None
    }

    fn normalizer_catalog(&self) -> NormalizerCatalog {
        NormalizerCatalog::empty("tic80.normalizer", env!("CARGO_PKG_VERSION"))
    }

    fn create_normalizer(
        &self,
        _requests: &[CapabilityRequest],
    ) -> Result<Option<Box<dyn Normalizer>>, String> {
        Ok(None)
    }

    fn prepare_run(
        &self,
        request: &glassvm_core::ExecutionRequest,
    ) -> Result<glassvm_core::PreparedRun, String> {
        request.validate_envelope()?;
        if request.machine_id != self.descriptor.id {
            return Err(format!(
                "execution request targets {}; bundle provides {}",
                request.machine_id, self.descriptor.id
            ));
        }
        tic80_core::parse_cart(&request.artifact)
            .map_err(|error| format!("invalid TIC-80 cartridge: {error}"))?;
        let prepared = glassvm_core::PreparedRun::prepare(
            request.run_id.clone(),
            &self.contract.artifact,
            &request.artifact,
            self.machine_identity(),
            &request.configuration,
            &self.emulator.config_schema(),
            &self.contract.inputs,
            &request.input_schedule,
            &request.execution_controls,
            &self.emulator.execution_limit_catalog(),
        )?;
        validate_frame_schedule(&prepared)?;
        Ok(prepared)
    }
}

impl Tic80Plugin {
    fn machine_identity(&self) -> glassvm_core::MachineIdentity {
        glassvm_core::MachineIdentity {
            machine_id: self.descriptor.id.clone(),
            bundle_version: self.descriptor.bundle_version.clone(),
            machine_version: self.descriptor.machine_version.clone(),
            emulator_version: self.descriptor.emulator_version.clone(),
            contract_schema: glassvm_core::SchemaRef::new(
                "glassvm.machine_contract",
                glassvm_core::SchemaVersion::V1,
            ),
        }
    }
}

fn validate_frame_schedule(prepared: &glassvm_core::PreparedRun) -> Result<(), String> {
    let Some(frame_limit) = prepared.execution_controls.frame_limit else {
        return Ok(());
    };
    for entry in &prepared.input_schedule.entries {
        let glassvm_core::InputCoordinate::Frame { frame } = &entry.coordinate else {
            return Err(format!(
                "TIC-80 input {} must use a frame coordinate",
                entry.input_id
            ));
        };
        if *frame >= frame_limit {
            return Err(format!(
                "TIC-80 input {} targets frame {frame}, outside frame limit {frame_limit}",
                entry.input_id
            ));
        }
    }
    Ok(())
}
