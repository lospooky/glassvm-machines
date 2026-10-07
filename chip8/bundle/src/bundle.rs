use glassvm_core::{
    CapabilityRequest, EmulatorBackend, MachineBundle, MachineContract, MachineDescriptor,
    Normalizer, StaticAnalyzerBackend, VerifierBackend,
};

use crate::adapters::{Chip8StaticAnalyzerBackend, Chip8VerifierBackend};
use crate::contract::chip8_contract;
use crate::descriptor::chip8_descriptor;
use crate::emulator_backend::Chip8EmulatorBackend;
use crate::normalizer::catalog as normalizer_catalog;
pub struct Chip8Plugin {
    descriptor: MachineDescriptor,
    contract: MachineContract,
    emulator: Chip8EmulatorBackend,
    analyzer: Chip8StaticAnalyzerBackend,
    verifier: Chip8VerifierBackend,
}

impl Chip8Plugin {
    pub fn new() -> Self {
        let contract = chip8_contract();
        Self {
            descriptor: chip8_descriptor(),
            contract,
            emulator: Chip8EmulatorBackend::new(),
            analyzer: Chip8StaticAnalyzerBackend,
            verifier: Chip8VerifierBackend,
        }
    }
}

impl Default for Chip8Plugin {
    fn default() -> Self {
        Self::new()
    }
}

impl MachineBundle for Chip8Plugin {
    fn descriptor(&self) -> &MachineDescriptor {
        &self.descriptor
    }

    fn contract(&self) -> &MachineContract {
        &self.contract
    }

    fn emulator(&self) -> &dyn EmulatorBackend {
        &self.emulator
    }

    fn static_analyzer(&self) -> Option<&dyn StaticAnalyzerBackend> {
        Some(&self.analyzer)
    }

    fn verifier(&self) -> Option<&dyn VerifierBackend> {
        Some(&self.verifier)
    }

    fn live_input_capability(&self) -> Option<&dyn glassvm_core::LiveInputCapability> {
        Some(&self.emulator)
    }

    fn normalizer_catalog(&self) -> glassvm_core::NormalizerCatalog {
        normalizer_catalog()
    }

    fn create_normalizer(
        &self,
        _requests: &[CapabilityRequest],
    ) -> Result<Option<Box<dyn Normalizer>>, String> {
        Ok(Some(Box::new(crate::normalizer::Chip8Normalizer::new())))
    }
}
