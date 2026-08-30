//! Hexwell: MachineBundle assembly; implementation concerns live in their named modules.

use glassvm_core::{
    CapabilityRequest, EmulatorBackend, MachineBundle, MachineContract, MachineDescriptor,
    Normalizer, StaticAnalyzerBackend, VerifierBackend,
};

use crate::adapters::{HexwellStaticAnalyzerBackend, HexwellVerifierBackend};
use crate::contract::contract;
use crate::descriptor::descriptor;
use crate::emulator_backend::HexwellEmulatorBackend;
use crate::normalizer::catalog as normalizer_catalog;

/// Complete Hexwell machine package for GlassVM.
pub struct HexwellPlugin {
    descriptor: MachineDescriptor,
    contract: MachineContract,
    emulator: HexwellEmulatorBackend,
    analyzer: HexwellStaticAnalyzerBackend,
    verifier: HexwellVerifierBackend,
}

impl HexwellPlugin {
    pub fn new() -> Self {
        let contract = contract();
        Self {
            descriptor: descriptor(),
            contract,
            emulator: HexwellEmulatorBackend,
            analyzer: HexwellStaticAnalyzerBackend,
            verifier: HexwellVerifierBackend,
        }
    }
}

impl Default for HexwellPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl MachineBundle for HexwellPlugin {
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

    fn normalizer_catalog(&self) -> glassvm_core::NormalizerCatalog {
        normalizer_catalog()
    }

    fn create_normalizer(
        &self,
        _requests: &[CapabilityRequest],
    ) -> Result<Option<Box<dyn Normalizer>>, String> {
        Ok(Some(Box::new(crate::normalizer::HexwellNormalizer::new())))
    }
}
