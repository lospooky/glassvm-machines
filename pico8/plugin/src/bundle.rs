use glassvm_core::{
    BodyProvider, EmulatorBackend, FixedBodyProvider, MachineBundle, MachineContract,
    MachineDescriptor, NativeEventAdapter, ObservableAdapter, StaticAnalyzerBackend,
    VerifierBackend, VersionStamp,
};

use crate::adapters::{
    Pico8NativeEventAdapter, Pico8ObservableAdapter, Pico8StaticAnalyzerBackend,
    Pico8VerifierBackend,
};
use crate::body::Pico8HeadlessBodyRuntime;
use crate::contract::contract;
use crate::descriptor::descriptor;
use crate::emulator_backend::Pico8EmulatorBackend;
pub struct Pico8Plugin {
    descriptor: MachineDescriptor,
    contract: MachineContract,
    headless_body: FixedBodyProvider<Pico8HeadlessBodyRuntime>,
    emulator: Pico8EmulatorBackend,
    analyzer: Pico8StaticAnalyzerBackend,
    verifier: Pico8VerifierBackend,
    native_event_adapter: Pico8NativeEventAdapter,
    observable_adapter: Pico8ObservableAdapter,
}

impl Pico8Plugin {
    pub fn new() -> Self {
        let contract = contract();
        let headless_body = FixedBodyProvider::new(
            contract.default_body.clone(),
            VersionStamp::from(env!("CARGO_PKG_VERSION")),
        );
        Self {
            descriptor: descriptor(),
            contract,
            headless_body,
            emulator: Pico8EmulatorBackend,
            analyzer: Pico8StaticAnalyzerBackend,
            verifier: Pico8VerifierBackend,
            native_event_adapter: Pico8NativeEventAdapter,
            observable_adapter: Pico8ObservableAdapter,
        }
    }
}

impl Default for Pico8Plugin {
    fn default() -> Self {
        Self::new()
    }
}

impl MachineBundle for Pico8Plugin {
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
        &self.native_event_adapter
    }

    fn observable_adapter(&self) -> &dyn ObservableAdapter {
        &self.observable_adapter
    }

    fn body_provider(&self, body_id: &str) -> Option<&dyn BodyProvider> {
        (body_id == self.headless_body.descriptor().id).then_some(&self.headless_body)
    }
}
