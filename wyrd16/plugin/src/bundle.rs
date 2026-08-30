//! `MachineBundle` assembly.

use glassvm_core::{
    CapabilityRequest, EmulatorBackend, MachineBundle, MachineContract, MachineDescriptor,
    Normalizer, StaticAnalyzerBackend, VerifierBackend,
};

use crate::{
    adapters::{Wyrd16StaticAnalyzerBackend, Wyrd16VerifierBackend},
    contract::contract,
    descriptor::descriptor,
    emulator_backend::Wyrd16EmulatorBackend,
    normalizer::catalog as normalizer_catalog,
};

/// The complete Wyrd-16 machine package.
pub struct Wyrd16Plugin {
    descriptor: MachineDescriptor,
    contract: MachineContract,
    emulator: Wyrd16EmulatorBackend,
    analyzer: Wyrd16StaticAnalyzerBackend,
    verifier: Wyrd16VerifierBackend,
}

impl Wyrd16Plugin {
    pub fn new() -> Self {
        Self {
            descriptor: descriptor(),
            contract: contract(),
            emulator: Wyrd16EmulatorBackend,
            analyzer: Wyrd16StaticAnalyzerBackend,
            verifier: Wyrd16VerifierBackend,
        }
    }
}

impl Default for Wyrd16Plugin {
    fn default() -> Self {
        Self::new()
    }
}

impl MachineBundle for Wyrd16Plugin {
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
        Ok(Some(Box::new(crate::normalizer::Wyrd16Normalizer::new())))
    }
}
