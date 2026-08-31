//! `MachineBundle` assembly for TIC-80.

use glassvm_core::{
    BodyProvider, EmulatorBackend, FixedBodyProvider, MachineBundle, MachineContract,
    MachineDescriptor, NativeEventAdapter, ObservableAdapter, StaticAnalyzerBackend,
    VerifierBackend,
};

use crate::adapters::{
    Tic80NativeEventAdapter, Tic80ObservableAdapter, Tic80StaticAnalyzer, Tic80Verifier,
};
use crate::body::Tic80BodyRuntime;
use crate::contract::contract;
use crate::descriptor::descriptor;
use crate::emulator_backend::Tic80EmulatorBackend;

pub struct Tic80Plugin {
    descriptor: MachineDescriptor,
    contract: MachineContract,
    body: FixedBodyProvider<Tic80BodyRuntime>,
    emulator: Tic80EmulatorBackend,
    analyzer: Tic80StaticAnalyzer,
    verifier: Tic80Verifier,
    native_events: Tic80NativeEventAdapter,
    observables: Tic80ObservableAdapter,
}

impl Tic80Plugin {
    pub fn new() -> Self {
        let descriptor = descriptor();
        let contract = contract();
        let body = FixedBodyProvider::new(
            contract.default_body.clone(),
            descriptor.bundle_version.clone(),
        );
        Self {
            descriptor,
            contract,
            body,
            emulator: Tic80EmulatorBackend,
            analyzer: Tic80StaticAnalyzer,
            verifier: Tic80Verifier,
            native_events: Tic80NativeEventAdapter,
            observables: Tic80ObservableAdapter,
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

    fn emulator(&self) -> &dyn EmulatorBackend {
        &self.emulator
    }

    fn static_analyzer(&self) -> Option<&dyn StaticAnalyzerBackend> {
        Some(&self.analyzer)
    }

    fn verifier(&self) -> Option<&dyn VerifierBackend> {
        Some(&self.verifier)
    }

    fn native_event_adapter(&self) -> &dyn NativeEventAdapter {
        &self.native_events
    }

    fn observable_adapter(&self) -> &dyn ObservableAdapter {
        &self.observables
    }

    fn body_provider(&self, body_id: &str) -> Option<&dyn BodyProvider> {
        (body_id == self.body.descriptor().id).then_some(&self.body)
    }
}
