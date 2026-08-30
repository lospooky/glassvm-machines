//! Hexwell: Session-bound snapshot encoding, validation, and continuation state.

use glassvm_core::{ContentDigest, ExecutionRequest, canonical_json_fingerprint};

use crate::emulator_session::HexwellSession;
use hexwell_core::{Catalyst, Family, ReactorState, Telemetry, WELL_COUNT};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionLifecycle {
    Fresh,
    Incremental,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContinuationContract {
    pub request: ExecutionRequest,
}

impl ContinuationContract {
    pub fn from_request(request: &ExecutionRequest) -> Self {
        Self {
            request: request.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionImage {
    pub snapshot_version: u16,
    pub contract: ContinuationContract,
    pub lifecycle: SessionLifecycle,
    pub reactor: ReactorState,
    pub telemetry: Telemetry,
    pub next_scheduled_input: usize,
    pub frame_open: bool,
    pub sweeps_into_frame: u32,
    pub payload_digest: ContentDigest,
}

impl SessionImage {
    fn computed_digest(&self) -> Result<ContentDigest, String> {
        let mut payload = serde_json::to_value(self)
            .map_err(|error| format!("encode Hexwell snapshot payload: {error}"))?;
        payload
            .as_object_mut()
            .ok_or("Hexwell snapshot payload is not an object")?
            .remove("payload_digest");
        canonical_json_fingerprint("glassvm.hexwell.session-snapshot.v4", &payload)
    }

    pub(crate) fn refresh_digest(&mut self) -> Result<(), String> {
        self.payload_digest = self.computed_digest()?;
        Ok(())
    }

    fn validate_digest(&self) -> Result<(), String> {
        if self.payload_digest != self.computed_digest()? {
            return Err("Hexwell snapshot payload digest mismatch".into());
        }
        Ok(())
    }
}

impl HexwellSession {
    pub(crate) fn snapshot_bytes_for(
        &self,
        lifecycle: SessionLifecycle,
    ) -> Result<Vec<u8>, String> {
        let mut image = self.image.clone();
        image.lifecycle = lifecycle;
        image.refresh_digest()?;
        serde_json::to_vec(&image).map_err(|error| error.to_string())
    }

    pub(crate) fn snapshot_bytes(&self) -> Result<Vec<u8>, String> {
        self.snapshot_bytes_for(self.image.lifecycle)
    }
}

pub(crate) fn validate_session_image(
    image: &SessionImage,
    scheduled_inputs: &[crate::emulator_backend::ScheduledTideInput],
    max_frames: u64,
    sweeps_per_frame: u32,
) -> Result<(), String> {
    let reactor = &image.reactor;
    let telemetry = &image.telemetry;
    image.validate_digest()?;

    if telemetry.coverage.len() != WELL_COUNT {
        return Err("snapshot coverage map does not contain 256 wells".into());
    }
    if telemetry.initial_matter != 38 {
        return Err("snapshot initial-matter ledger is not the Hexwell boot budget of 38".into());
    }
    if !image.frame_open && image.sweeps_into_frame != 0 {
        return Err("snapshot has frame-boundary state with an in-frame sweep count".into());
    }
    if image.sweeps_into_frame > sweeps_per_frame {
        return Err("snapshot in-frame sweep count exceeds sweeps_per_frame".into());
    }
    if reactor.frames > max_frames {
        return Err("snapshot frame count exceeds the execution frame budget".into());
    }
    if u64::from(image.sweeps_into_frame) > reactor.sweeps {
        return Err("snapshot in-frame sweep count exceeds total completed sweeps".into());
    }
    if image.frame_open && image.sweeps_into_frame == 0 {
        return Err("snapshot claims an open frame before any sweep checkpoint".into());
    }
    if reactor.sweeps == 0 && reactor.quenched() {
        return Err("snapshot has lost the mandatory boot spark before any sweep".into());
    }

    let expected_cursor = scheduled_inputs
        .iter()
        .take_while(|stimulus| {
            let frame = stimulus.frame;
            frame < reactor.frames || (image.frame_open && frame == reactor.frames)
        })
        .count();
    if image.next_scheduled_input != expected_cursor {
        return Err(format!(
            "snapshot scheduled-input cursor is {}; frame phase requires {expected_cursor}",
            image.next_scheduled_input
        ));
    }

    let coverage_total: u128 = telemetry
        .coverage
        .iter()
        .map(|count| u128::from(*count))
        .sum();
    let opcode_total: u128 = telemetry
        .opcode_counts
        .iter()
        .map(|count| u128::from(*count))
        .sum();
    if coverage_total != u128::from(reactor.firings) || opcode_total != u128::from(reactor.firings)
    {
        return Err("snapshot coverage/opcode telemetry does not equal completed firings".into());
    }
    if telemetry
        .coverage
        .iter()
        .any(|count| *count > reactor.sweeps)
    {
        return Err("snapshot per-well coverage exceeds completed sweeps".into());
    }
    if reactor.firings < reactor.sweeps
        || u128::from(reactor.firings) > u128::from(reactor.sweeps) * WELL_COUNT as u128
    {
        return Err("snapshot firing count is inconsistent with completed sweeps".into());
    }
    if reactor.frames > reactor.sweeps {
        return Err("snapshot frame count exceeds completed sweeps".into());
    }
    let minimum_sweeps = u128::from(reactor.frames) + u128::from(image.sweeps_into_frame);
    if u128::from(reactor.sweeps) < minimum_sweeps {
        return Err("snapshot sweep count is below its completed and open frame minimum".into());
    }
    let sweep_ceiling = u128::from(reactor.frames) * u128::from(sweeps_per_frame)
        + u128::from(image.sweeps_into_frame);
    if u128::from(reactor.sweeps) > sweep_ceiling {
        return Err("snapshot sweep count exceeds its completed and open frame budgets".into());
    }
    if image.frame_open || !reactor.quenched() {
        if u128::from(reactor.sweeps) != sweep_ceiling {
            return Err(
                "snapshot active/open frame phase does not exactly partition completed sweeps"
                    .into(),
            );
        }
    } else {
        if reactor.frames == 0 {
            return Err("snapshot is quenched before any completed frame".into());
        }
        let earliest_quench = u128::from(reactor.frames - 1) * u128::from(sweeps_per_frame) + 1;
        if u128::from(reactor.sweeps) < earliest_quench {
            return Err("snapshot quenched-frame sweep count is below its possible minimum".into());
        }
    }

    if telemetry.vents != telemetry.vented_matter {
        return Err("snapshot vent counter and vented-matter ledger disagree".into());
    }
    let mut derived_opcode_counts = [0_u128; 16];
    for (well, count) in telemetry.coverage.iter().enumerate() {
        let family = Catalyst::decode(reactor.plate[well]).family as usize;
        derived_opcode_counts[family] += u128::from(*count);
    }
    if derived_opcode_counts
        .iter()
        .zip(telemetry.opcode_counts)
        .any(|(derived, recorded)| *derived != u128::from(recorded))
    {
        return Err("snapshot opcode counts do not match plate-weighted coverage".into());
    }
    if telemetry.precipitations > telemetry.opcode_counts[Family::Precipitate as usize]
        || telemetry.dissolutions > telemetry.opcode_counts[Family::Dissolve as usize]
        || telemetry.bindings > telemetry.opcode_counts[Family::Bind as usize]
        || telemetry.cleavages > telemetry.opcode_counts[Family::Cleave as usize]
        || telemetry.tinctures > telemetry.opcode_counts[Family::Tincture as usize]
        || telemetry.vents > telemetry.opcode_counts[Family::Vent as usize]
    {
        return Err("snapshot successful-reaction counts exceed matching catalyst firings".into());
    }
    let transport_firings = u128::from(telemetry.opcode_counts[Family::Drip as usize])
        + u128::from(telemetry.opcode_counts[Family::Pour as usize])
        + u128::from(telemetry.opcode_counts[Family::Osmose as usize]);
    if telemetry.transfer_quanta_accepted > telemetry.transfer_quanta_requested
        || u128::from(telemetry.transfer_requests) > transport_firings
        || u128::from(telemetry.transfer_quanta_requested)
            > u128::from(telemetry.transfer_requests) * 8
    {
        return Err("snapshot transfer telemetry is internally inconsistent".into());
    }
    let local_effects = u128::from(telemetry.precipitations)
        + u128::from(telemetry.dissolutions)
        + u128::from(telemetry.bindings)
        + u128::from(telemetry.cleavages)
        + u128::from(telemetry.tinctures)
        + u128::from(telemetry.vents);
    if local_effects > u128::from(reactor.firings) {
        return Err("snapshot reaction telemetry exceeds completed firings".into());
    }
    if telemetry.peak_active_wells == 0
        || telemetry.peak_active_wells > WELL_COUNT
        || telemetry.peak_active_wells < reactor.active_count()
    {
        return Err("snapshot peak-front telemetry is outside its valid bounds".into());
    }
    let fed_frames = u128::from(reactor.frames) + u128::from(u8::from(image.frame_open));
    if u128::from(telemetry.introduced_matter) > fed_frames * 6
        || telemetry.vented_matter > reactor.firings
    {
        return Err("snapshot open-system counters exceed possible feeds or firings".into());
    }

    let actual = reactor.total_matter() as i128;
    if actual != telemetry.expected_matter() {
        return Err(format!(
            "snapshot violates matter ledger: field has {actual}, ledger expects {}",
            telemetry.expected_matter()
        ));
    }
    Ok(())
}
