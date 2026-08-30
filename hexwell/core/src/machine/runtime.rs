//! Native Hexwell reactor state and deterministic sweep semantics.

use serde::{Deserialize, Serialize};

use super::instruction::{
    CENTER_WELL, Catalyst, Family, Materia, WELL_COUNT, neighbor, opposite, portal,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WellState {
    pub brine: u8,
    pub ember: u8,
    pub crystal: u8,
    pub heat: u8,
    pub spark: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReactorState {
    pub plate: Vec<u8>,
    pub brine: Vec<u8>,
    pub ember: Vec<u8>,
    pub crystal: Vec<u8>,
    pub heat: Vec<u8>,
    pub sparks: Vec<bool>,
    pub tide: u8,
    pub sweeps: u64,
    pub frames: u64,
    pub firings: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Telemetry {
    pub coverage: Vec<u64>,
    pub opcode_counts: [u64; 16],
    pub transfer_requests: u64,
    pub transfer_quanta_requested: u64,
    pub transfer_quanta_accepted: u64,
    pub precipitations: u64,
    pub dissolutions: u64,
    pub bindings: u64,
    pub cleavages: u64,
    pub tinctures: u64,
    pub vents: u64,
    pub introduced_matter: u64,
    pub vented_matter: u64,
    pub peak_active_wells: usize,
    pub initial_matter: u64,
}

impl Telemetry {
    pub fn boot(initial_matter: u64) -> Self {
        Self {
            coverage: vec![0; WELL_COUNT],
            opcode_counts: [0; 16],
            transfer_requests: 0,
            transfer_quanta_requested: 0,
            transfer_quanta_accepted: 0,
            precipitations: 0,
            dissolutions: 0,
            bindings: 0,
            cleavages: 0,
            tinctures: 0,
            vents: 0,
            introduced_matter: 0,
            vented_matter: 0,
            peak_active_wells: 1,
            initial_matter,
        }
    }

    pub fn absorb_sweep(&mut self, outcome: &SweepOutcome) -> Result<(), String> {
        let mut next = self.clone();
        for firing in &outcome.firings {
            next.coverage[firing.well] =
                checked_counter_add(next.coverage[firing.well], 1, "per-well coverage counter")?;
            let family = firing.catalyst.family as usize;
            next.opcode_counts[family] =
                checked_counter_add(next.opcode_counts[family], 1, "opcode-family counter")?;
        }
        next.transfer_requests = checked_counter_add(
            next.transfer_requests,
            outcome.transfers.len() as u64,
            "transfer-request counter",
        )?;
        let requested = outcome
            .transfers
            .iter()
            .map(|transfer| u64::from(transfer.requested))
            .sum::<u64>();
        next.transfer_quanta_requested = checked_counter_add(
            next.transfer_quanta_requested,
            requested,
            "requested-transfer counter",
        )?;
        let accepted = outcome
            .transfers
            .iter()
            .map(|transfer| u64::from(transfer.accepted))
            .sum::<u64>();
        next.transfer_quanta_accepted = checked_counter_add(
            next.transfer_quanta_accepted,
            accepted,
            "accepted-transfer counter",
        )?;
        next.precipitations = checked_counter_add(
            next.precipitations,
            outcome.precipitations,
            "precipitation counter",
        )?;
        next.dissolutions = checked_counter_add(
            next.dissolutions,
            outcome.dissolutions,
            "dissolution counter",
        )?;
        next.bindings = checked_counter_add(next.bindings, outcome.bindings, "binding counter")?;
        next.cleavages =
            checked_counter_add(next.cleavages, outcome.cleavages, "cleavage counter")?;
        next.tinctures =
            checked_counter_add(next.tinctures, outcome.tinctures, "tincture counter")?;
        next.vents = checked_counter_add(next.vents, outcome.vented, "vent counter")?;
        next.vented_matter =
            checked_counter_add(next.vented_matter, outcome.vented, "vented-matter ledger")?;
        next.peak_active_wells = next
            .peak_active_wells
            .max(outcome.active_before)
            .max(outcome.active_after);
        *self = next;
        Ok(())
    }

    pub fn expected_matter(&self) -> i128 {
        i128::from(self.initial_matter) + i128::from(self.introduced_matter)
            - i128::from(self.vented_matter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellChange {
    pub well: usize,
    pub before: WellState,
    pub after: WellState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiringRecord {
    pub well: usize,
    pub catalyst: Catalyst,
    pub spark_targets: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferRecord {
    pub from: usize,
    pub to: usize,
    pub materia: Materia,
    pub requested: u8,
    pub accepted: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepOutcome {
    pub sweep: u64,
    pub firings: Vec<FiringRecord>,
    pub transfers: Vec<TransferRecord>,
    pub changes: Vec<CellChange>,
    pub active_before: usize,
    pub active_after: usize,
    pub matter_before: u64,
    pub matter_after: u64,
    pub precipitations: u64,
    pub dissolutions: u64,
    pub bindings: u64,
    pub cleavages: u64,
    pub tinctures: u64,
    pub vented: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedOutcome {
    pub changes: Vec<CellChange>,
    pub introduced: u64,
    pub ignited: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TransferIntent {
    from: usize,
    to: usize,
    materia: Materia,
    requested: u8,
}

impl ReactorState {
    pub fn boot(plate: &[u8], seed: u64) -> Result<Self, String> {
        if plate.len() != WELL_COUNT {
            return Err(format!(
                "Hexwell ROM must contain exactly {WELL_COUNT} catalyst bytes; got {}",
                plate.len()
            ));
        }

        let mut state = Self {
            plate: plate.to_vec(),
            brine: vec![0; WELL_COUNT],
            ember: vec![0; WELL_COUNT],
            crystal: vec![0; WELL_COUNT],
            heat: vec![0; WELL_COUNT],
            sparks: vec![false; WELL_COUNT],
            tide: 0,
            sweeps: 0,
            frames: 0,
            firings: 0,
        };

        let seed_bytes = seed.to_le_bytes();
        let brine_well = neighbor(CENTER_WELL, seed_bytes[0] % 6);
        let ember_well = neighbor(CENTER_WELL, seed_bytes[1] % 6);
        let crystal_well = neighbor(CENTER_WELL, seed_bytes[2] % 6);
        state.brine[brine_well] = 15;
        state.ember[ember_well] = 15;
        state.crystal[crystal_well] = 8;
        state.heat[CENTER_WELL] = 8;
        state.sparks[CENTER_WELL] = true;
        Ok(state)
    }

    pub fn validate(&self) -> Result<(), String> {
        for (name, len) in [
            ("plate", self.plate.len()),
            ("brine", self.brine.len()),
            ("ember", self.ember.len()),
            ("crystal", self.crystal.len()),
            ("heat", self.heat.len()),
            ("sparks", self.sparks.len()),
        ] {
            if len != WELL_COUNT {
                return Err(format!(
                    "snapshot {name} field has {len} wells; expected {WELL_COUNT}"
                ));
            }
        }
        for (name, field) in [
            ("brine", &self.brine),
            ("ember", &self.ember),
            ("crystal", &self.crystal),
            ("heat", &self.heat),
        ] {
            if let Some((well, value)) = field
                .iter()
                .copied()
                .enumerate()
                .find(|(_, value)| *value > 15)
            {
                return Err(format!(
                    "snapshot {name} concentration {value} at well {well} exceeds 15"
                ));
            }
        }
        Ok(())
    }

    pub fn well(&self, index: usize) -> WellState {
        WellState {
            brine: self.brine[index],
            ember: self.ember[index],
            crystal: self.crystal[index],
            heat: self.heat[index],
            spark: self.sparks[index],
        }
    }

    pub fn total_matter(&self) -> u64 {
        self.brine
            .iter()
            .chain(&self.ember)
            .chain(&self.crystal)
            .map(|value| u64::from(*value))
            .sum()
    }

    pub fn active_count(&self) -> usize {
        self.sparks.iter().filter(|spark| **spark).count()
    }

    pub fn quenched(&self) -> bool {
        self.active_count() == 0
    }

    pub fn set_tide(&mut self, tide: u8) {
        self.tide = tide;
    }

    pub fn feed_tide(&mut self) -> FeedOutcome {
        let mut before = Vec::new();
        let materia = if self.tide & 0x40 == 0 {
            Materia::Brine
        } else {
            Materia::Ember
        };
        let ignite = self.tide & 0x80 != 0;
        let mut introduced = 0_u64;
        let mut ignited = 0_usize;

        for direction in 0..6 {
            if self.tide & (1 << direction) == 0 {
                continue;
            }
            let well = portal(direction);
            let old = self.well(well);
            before.push((well, old));
            let channel = match materia {
                Materia::Brine => &mut self.brine,
                Materia::Ember => &mut self.ember,
            };
            if channel[well] < 15 {
                channel[well] += 1;
                introduced += 1;
            }
            if ignite && !self.sparks[well] {
                self.sparks[well] = true;
                ignited += 1;
            }
        }

        FeedOutcome {
            changes: before
                .into_iter()
                .filter_map(|(well, old)| {
                    let new = self.well(well);
                    (old != new).then_some(CellChange {
                        well,
                        before: old,
                        after: new,
                    })
                })
                .collect(),
            introduced,
            ignited,
        }
    }

    pub fn finish_frame(&mut self) -> Result<Vec<CellChange>, String> {
        let completed_frames = self
            .frames
            .checked_add(1)
            .ok_or_else(|| "Hexwell completed-frame counter exhausted u64".to_string())?;
        let mut changes = Vec::new();
        for well in 0..WELL_COUNT {
            if self.heat[well] == 0 {
                continue;
            }
            let before = self.well(well);
            self.heat[well] -= 1;
            changes.push(CellChange {
                well,
                before,
                after: self.well(well),
            });
        }
        self.frames = completed_frames;
        Ok(changes)
    }

    pub fn sweep(&mut self) -> Result<SweepOutcome, String> {
        let old_brine = self.brine.clone();
        let old_ember = self.ember.clone();
        let old_crystal = self.crystal.clone();
        let old_heat = self.heat.clone();
        let old_sparks = self.sparks.clone();
        let active: Vec<usize> = old_sparks
            .iter()
            .enumerate()
            .filter_map(|(well, spark)| (*spark).then_some(well))
            .collect();
        let completed_sweeps = self
            .sweeps
            .checked_add(1)
            .ok_or_else(|| "Hexwell completed-sweep counter exhausted u64".to_string())?;
        let completed_firings = self
            .firings
            .checked_add(active.len() as u64)
            .ok_or_else(|| "Hexwell completed-firing counter exhausted u64".to_string())?;
        let matter_before = field_total(&old_brine, &old_ember, &old_crystal);

        let mut next_brine = old_brine.clone();
        let mut next_ember = old_ember.clone();
        let mut next_crystal = old_crystal.clone();
        let mut next_heat = old_heat.clone();
        let mut next_sparks = vec![false; WELL_COUNT];
        let mut intents = Vec::new();
        let mut firings = Vec::with_capacity(active.len());
        let mut precipitations = 0;
        let mut dissolutions = 0;
        let mut bindings = 0;
        let mut cleavages = 0;
        let mut tinctures = 0;
        let mut vented = 0;

        for &well in &active {
            let catalyst = Catalyst::decode(self.plate[well]);
            let materia = catalyst.materia();
            let target =
                self.resolve_target(well, catalyst.selector(), materia, &old_brine, &old_ember);
            let mut spark_targets = Vec::new();

            match catalyst.family {
                Family::Dormant => {
                    if catalyst.polarity() {
                        spark_targets.push(well);
                    }
                }
                Family::Drip | Family::Pour | Family::Osmose => {
                    let source = channel_value(materia, &old_brine, &old_ember, well);
                    let destination = channel_value(materia, &old_brine, &old_ember, target);
                    let requested = match catalyst.family {
                        Family::Drip => u8::from(source > 0),
                        Family::Pour => source.min(4),
                        Family::Osmose => {
                            if source > destination {
                                (source - destination).div_ceil(2)
                            } else {
                                0
                            }
                        }
                        _ => unreachable!(),
                    };
                    if target != well {
                        intents.push(TransferIntent {
                            from: well,
                            to: target,
                            materia,
                            requested,
                        });
                    }
                    spark_targets.push(target);
                }
                Family::Precipitate => {
                    let selected = channel_value(materia, &old_brine, &old_ember, well);
                    if selected > 0 && old_crystal[well] < 15 {
                        decrement_channel(materia, &mut next_brine, &mut next_ember, well);
                        next_crystal[well] += 1;
                        next_heat[well] = old_heat[well].saturating_add(1).min(15);
                        precipitations += 1;
                    }
                    spark_targets.push(target);
                }
                Family::Dissolve => {
                    let selected = channel_value(materia, &old_brine, &old_ember, well);
                    if old_crystal[well] > 0 && selected < 15 && old_heat[well] > 0 {
                        next_crystal[well] -= 1;
                        increment_channel(materia, &mut next_brine, &mut next_ember, well);
                        next_heat[well] -= 1;
                        dissolutions += 1;
                    }
                    spark_targets.push(target);
                }
                Family::Bind => {
                    if old_brine[well] > 0 && old_ember[well] > 0 && old_crystal[well] <= 13 {
                        next_brine[well] -= 1;
                        next_ember[well] -= 1;
                        next_crystal[well] += 2;
                        next_heat[well] = old_heat[well].saturating_add(1).min(15);
                        bindings += 1;
                    }
                    spark_targets.push(target);
                }
                Family::Cleave => {
                    if old_crystal[well] >= 2
                        && old_brine[well] < 15
                        && old_ember[well] < 15
                        && old_heat[well] > 0
                    {
                        next_crystal[well] -= 2;
                        next_brine[well] += 1;
                        next_ember[well] += 1;
                        next_heat[well] -= 1;
                        cleavages += 1;
                    }
                    spark_targets.push(target);
                }
                Family::Tincture => {
                    let selected = channel_value(materia, &old_brine, &old_ember, well);
                    let other = channel_value(materia.other(), &old_brine, &old_ember, well);
                    if selected > 0 && other < 15 {
                        decrement_channel(materia, &mut next_brine, &mut next_ember, well);
                        increment_channel(materia.other(), &mut next_brine, &mut next_ember, well);
                        tinctures += 1;
                    }
                    spark_targets.push(target);
                }
                Family::Temper => {
                    let selected = channel_value(materia, &old_brine, &old_ember, well);
                    let other = channel_value(materia.other(), &old_brine, &old_ember, well);
                    next_heat[well] = if selected > other {
                        old_heat[well].saturating_add(1).min(15)
                    } else {
                        old_heat[well].saturating_sub(1)
                    };
                    spark_targets.push(target);
                }
                Family::Kindle => {
                    let amount = if catalyst.polarity() { 4 } else { 1 };
                    next_heat[well] = old_heat[well].saturating_add(amount).min(15);
                    spark_targets.push(target);
                }
                Family::Quench => {
                    let amount = if catalyst.polarity() { 4 } else { 1 };
                    next_heat[well] = old_heat[well].saturating_sub(amount);
                    spark_targets.push(target);
                }
                Family::Affinity => {
                    let here = channel_value(materia, &old_brine, &old_ember, well);
                    let there = channel_value(materia, &old_brine, &old_ember, target);
                    if here > there {
                        spark_targets.push(target);
                    }
                }
                Family::Fork => {
                    spark_targets.push(target);
                    if target != well {
                        let direction = direction_to_neighbor(well, target)
                            .expect("resolved non-self target is adjacent");
                        spark_targets.push(neighbor(well, opposite(direction)));
                    }
                }
                Family::Seek => {
                    spark_targets.push(self.seek_target(
                        well,
                        catalyst.selector(),
                        materia,
                        &old_brine,
                        &old_ember,
                    ));
                }
                Family::Vent => {
                    let selected = channel_value(materia, &old_brine, &old_ember, well);
                    if selected > 0 {
                        decrement_channel(materia, &mut next_brine, &mut next_ember, well);
                        vented += 1;
                    }
                    spark_targets.push(target);
                }
            }

            spark_targets.sort_unstable();
            spark_targets.dedup();
            for &spark_target in &spark_targets {
                next_sparks[spark_target] = true;
            }
            firings.push(FiringRecord {
                well,
                catalyst,
                spark_targets,
            });
        }

        intents.sort_by_key(|intent| {
            (
                intent.to,
                materia_order(intent.materia),
                incoming_rank(intent.to, intent.from),
                intent.from,
            )
        });
        let transfer_base_brine = next_brine.clone();
        let transfer_base_ember = next_ember.clone();
        let mut brine_capacity = transfer_base_brine
            .iter()
            .map(|value| 15 - value)
            .collect::<Vec<_>>();
        let mut ember_capacity = transfer_base_ember
            .iter()
            .map(|value| 15 - value)
            .collect::<Vec<_>>();
        let mut transfers = Vec::with_capacity(intents.len());
        for intent in intents {
            let capacity = match intent.materia {
                Materia::Brine => brine_capacity[intent.to],
                Materia::Ember => ember_capacity[intent.to],
            };
            let available = channel_value(
                intent.materia,
                &transfer_base_brine,
                &transfer_base_ember,
                intent.from,
            );
            let accepted = intent.requested.min(capacity).min(available);
            match intent.materia {
                Materia::Brine => brine_capacity[intent.to] -= accepted,
                Materia::Ember => ember_capacity[intent.to] -= accepted,
            }
            transfers.push(TransferRecord {
                from: intent.from,
                to: intent.to,
                materia: intent.materia,
                requested: intent.requested,
                accepted,
            });
        }
        for transfer in &transfers {
            if transfer.accepted == 0 {
                continue;
            }
            subtract_channel(
                transfer.materia,
                &mut next_brine,
                &mut next_ember,
                transfer.from,
                transfer.accepted,
            );
            add_channel(
                transfer.materia,
                &mut next_brine,
                &mut next_ember,
                transfer.to,
                transfer.accepted,
            );
        }

        let mut changes = Vec::new();
        for well in 0..WELL_COUNT {
            let before = WellState {
                brine: old_brine[well],
                ember: old_ember[well],
                crystal: old_crystal[well],
                heat: old_heat[well],
                spark: old_sparks[well],
            };
            let after = WellState {
                brine: next_brine[well],
                ember: next_ember[well],
                crystal: next_crystal[well],
                heat: next_heat[well],
                spark: next_sparks[well],
            };
            if before != after {
                changes.push(CellChange {
                    well,
                    before,
                    after,
                });
            }
        }

        self.brine = next_brine;
        self.ember = next_ember;
        self.crystal = next_crystal;
        self.heat = next_heat;
        self.sparks = next_sparks;
        self.sweeps = completed_sweeps;
        self.firings = completed_firings;
        let matter_after = self.total_matter();

        Ok(SweepOutcome {
            sweep: self.sweeps - 1,
            firings,
            transfers,
            changes,
            active_before: active.len(),
            active_after: self.active_count(),
            matter_before,
            matter_after,
            precipitations,
            dissolutions,
            bindings,
            cleavages,
            tinctures,
            vented,
        })
    }

    pub fn render_rgb332(&self) -> Vec<u8> {
        (0..WELL_COUNT)
            .map(|well| {
                let spark = u8::from(self.sparks[well]);
                let red =
                    (self.ember[well] / 2 + self.heat[well] / 4 + self.crystal[well] / 8 + spark)
                        .min(7);
                let green =
                    (self.ember[well] / 4 + self.brine[well] / 4 + self.crystal[well] / 4 + spark)
                        .min(7);
                let blue = (self.brine[well] / 4 + self.crystal[well] / 8 + spark).min(3);
                (red << 5) | (green << 2) | blue
            })
            .collect()
    }

    pub fn phenotype_hash(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for well in 0..WELL_COUNT {
            for byte in [
                self.brine[well],
                self.ember[well],
                self.crystal[well],
                self.heat[well],
                u8::from(self.sparks[well]),
            ] {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
            }
        }
        hash
    }

    fn resolve_target(
        &self,
        well: usize,
        selector: u8,
        materia: Materia,
        brine: &[u8],
        ember: &[u8],
    ) -> usize {
        match selector & 7 {
            direction @ 0..=5 => neighbor(well, direction),
            6 => well,
            7 => {
                let mut best_direction = 0;
                let mut best_value = channel_value(materia, brine, ember, neighbor(well, 0));
                for direction in 1..6 {
                    let value = channel_value(materia, brine, ember, neighbor(well, direction));
                    if value < best_value {
                        best_direction = direction;
                        best_value = value;
                    }
                }
                neighbor(well, best_direction)
            }
            _ => unreachable!("selector is masked"),
        }
    }

    fn seek_target(
        &self,
        well: usize,
        priority: u8,
        materia: Materia,
        brine: &[u8],
        ember: &[u8],
    ) -> usize {
        let start = priority % 6;
        let mut best_direction = start;
        let mut best_value = channel_value(materia, brine, ember, neighbor(well, start));
        for offset in 1..6 {
            let direction = (start + offset) % 6;
            let value = channel_value(materia, brine, ember, neighbor(well, direction));
            if value > best_value {
                best_direction = direction;
                best_value = value;
            }
        }
        neighbor(well, best_direction)
    }
}

fn channel_value(materia: Materia, brine: &[u8], ember: &[u8], well: usize) -> u8 {
    match materia {
        Materia::Brine => brine[well],
        Materia::Ember => ember[well],
    }
}

fn increment_channel(materia: Materia, brine: &mut [u8], ember: &mut [u8], well: usize) {
    match materia {
        Materia::Brine => brine[well] += 1,
        Materia::Ember => ember[well] += 1,
    }
}

fn decrement_channel(materia: Materia, brine: &mut [u8], ember: &mut [u8], well: usize) {
    match materia {
        Materia::Brine => brine[well] -= 1,
        Materia::Ember => ember[well] -= 1,
    }
}

fn add_channel(materia: Materia, brine: &mut [u8], ember: &mut [u8], well: usize, amount: u8) {
    match materia {
        Materia::Brine => brine[well] += amount,
        Materia::Ember => ember[well] += amount,
    }
}

fn subtract_channel(materia: Materia, brine: &mut [u8], ember: &mut [u8], well: usize, amount: u8) {
    match materia {
        Materia::Brine => brine[well] -= amount,
        Materia::Ember => ember[well] -= amount,
    }
}

fn materia_order(materia: Materia) -> u8 {
    match materia {
        Materia::Brine => 0,
        Materia::Ember => 1,
    }
}

fn direction_to_neighbor(well: usize, target: usize) -> Option<u8> {
    (0..6).find(|direction| neighbor(well, *direction) == target)
}

fn incoming_rank(target: usize, source: usize) -> u8 {
    direction_to_neighbor(target, source).unwrap_or(6)
}

fn field_total(brine: &[u8], ember: &[u8], crystal: &[u8]) -> u64 {
    brine
        .iter()
        .chain(ember)
        .chain(crystal)
        .map(|value| u64::from(*value))
        .sum()
}

fn checked_counter_add(value: u64, increment: u64, name: &str) -> Result<u64, String> {
    value
        .checked_add(increment)
        .ok_or_else(|| format!("Hexwell {name} exhausted u64"))
}

pub fn connected_matter_components(state: &ReactorState) -> usize {
    let occupied: Vec<bool> = (0..WELL_COUNT)
        .map(|well| state.brine[well] + state.ember[well] + state.crystal[well] > 0)
        .collect();
    let mut visited = vec![false; WELL_COUNT];
    let mut components = 0;
    for start in 0..WELL_COUNT {
        if !occupied[start] || visited[start] {
            continue;
        }
        components += 1;
        let mut stack = vec![start];
        visited[start] = true;
        while let Some(well) = stack.pop() {
            for direction in 0..6 {
                let adjacent = neighbor(well, direction);
                if occupied[adjacent] && !visited[adjacent] {
                    visited[adjacent] = true;
                    stack.push(adjacent);
                }
            }
        }
    }
    components
}
