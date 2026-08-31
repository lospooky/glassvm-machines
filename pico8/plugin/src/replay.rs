use glassvm_core::{
    Emission, EmissionSink, ExecutionRequest, ReplayComparison, ReplayReceipt,
    ResolvedEpisodeContext, RunResult, fixed_body_action_schema,
    materialize_open_loop_episode_context,
};

use crate::contract::contract;
use crate::identity::{pico8_headless_body_identity, recorded_controller_policy_identity};

pub(super) fn resolve_episode_context(
    request: &ExecutionRequest,
) -> Result<ResolvedEpisodeContext, String> {
    let body = pico8_headless_body_identity();
    let action_schema = fixed_body_action_schema(&contract().default_body);
    let recorded_input = recorded_controller_policy_identity();
    materialize_open_loop_episode_context(
        request.episode.as_ref(),
        &request.stimuli,
        &body,
        &action_schema,
        Some(&recorded_input),
    )
}

#[derive(Debug, Clone)]
pub struct Pico8ReplayOutcome {
    pub preflight: ReplayComparison,
    pub result: Option<RunResult>,
    pub receipt: Option<ReplayReceipt>,
    pub outcome: Option<ReplayComparison>,
}

impl Pico8ReplayOutcome {
    pub fn is_match(&self) -> bool {
        self.preflight.is_match()
            && self
                .outcome
                .as_ref()
                .is_some_and(ReplayComparison::is_match)
    }
}

pub(super) struct ReceiptTap<'a> {
    pub(super) downstream: &'a mut dyn EmissionSink,
    pub(super) receipt: Option<ReplayReceipt>,
}

impl EmissionSink for ReceiptTap<'_> {
    fn emit(&mut self, emission: Emission<'_>) -> Result<(), glassvm_core::SinkError> {
        if let Emission::ReplayReceipt(receipt) = &emission {
            self.receipt = Some((*receipt).clone());
        }
        self.downstream.emit(emission)
    }
}
