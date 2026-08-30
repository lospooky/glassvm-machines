use crate::display::{BUF_SIZE, HIRES_H, HIRES_W, LORES_H, LORES_W};
use crate::event::{Event, Timer};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// Canonical byte encoding shared with `chip8_visual_audit`.
pub const TRAJECTORY_IDENTITY_DEFINITION: &str =
    "ordered 128x64 physical composite frames; row-major u8 plane values; no separators";

/// Exact identity of all completed physical display frames in a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrajectoryIdentity {
    /// Canonical byte encoding hashed by this identity.
    pub definition: &'static str,
    /// Lowercase hexadecimal SHA-256 digest of the encoded frames.
    pub digest: String,
    /// Number of completed frames represented by `digest`.
    pub frame_count: u64,
}

/// Interestingness metrics computed after a completed execution run.
///
/// Fields comprise bounded ratios, non-negative counts and indices, and a few
/// unbounded-positive floats. Zero typically means "not enough data" or
/// "feature absent"; field-specific documentation records any ambiguity.
#[derive(Debug, Clone, Default)]
pub struct InterestingnessSummary {
    /// Shannon entropy of the per-frame framebuffer hash distribution.
    /// High → many visually distinct frames; 0 → screen never changes.
    pub frame_entropy: f32,
    /// Fraction of consecutive frame-pairs that differed (0 – 1).
    pub change_rate: f32,
    /// Zero-based index of the final frame whose hash differs from its
    /// predecessor. Zero also represents a trajectory with no changed
    /// transition.
    pub last_change_frame: u32,
    /// Fraction of changed transitions in the final quarter of the completed
    /// transition sequence.
    pub late_change_rate: f32,
    /// Fraction of frames in the final quarter whose hash appears for the
    /// first time anywhere in the trajectory at that frame.
    pub late_frame_discovery_rate: f32,
    /// Normalised Shannon entropy over the 16 opcode-class histogram (0 – 1).
    pub opcode_diversity: f32,
    /// `unique_pcs / sqrt(total_cycles)` — exploration rate.
    pub coverage_growth: f32,
    /// Fraction of input-events after which the screen changed (0 – 1).
    pub input_responsiveness: f32,
    /// Draw instructions per cycle.
    pub draw_density: f32,
    /// Mean fraction of physical display pixels with a non-zero composite
    /// colour. Lores pixels occupy 2×2 backing pixels, so the normalised value
    /// is resolution-independent.
    pub mean_lit_fraction: f32,
    /// Largest lit-pixel fraction observed in any frame.
    pub peak_lit_fraction: f32,
    /// Mean fraction of composite-colour pixels that changed between
    /// consecutive frames.
    pub mean_frame_delta: f32,
    /// Mean physical composite-colour pixels changed between consecutive
    /// frames, with each transition capped at 48 pixels. Zero-change
    /// transitions are included in the mean.
    pub mean_capped_changed_pixels: f32,
    /// Fraction of active framebuffer transitions whose changed pixels touch
    /// at least six physical rows and twelve physical columns simultaneously.
    /// Zero-change transitions are excluded from the denominator.
    pub broad_transition_share: f32,
    /// Normalised Shannon entropy of changed-pixel energy across up to 12
    /// equal timeline windows. Zero means motion is absent or concentrated in
    /// one window; one means changed-pixel energy is spread uniformly.
    pub motion_spread: f32,
    /// Mean normalised Shannon entropy of changed-pixel energy across the
    /// physical display's fixed 8×4 region grid, measured in up to 12 equal
    /// timeline windows. Zero means motion is absent or spatially confined;
    /// one means changed-pixel energy is uniform over the grid throughout.
    pub motion_spatial_spread: f32,
    /// Mean of the weaker normalised row/column changed-energy entropy over
    /// the same timeline windows as `motion_spatial_spread`. Zero means
    /// motion is absent or confined to one physical-display axis; one means
    /// changed-pixel energy is balanced across both axes throughout.
    pub motion_axis_balance: f32,
    /// Coefficient of variation of per-frame changed-pixel fractions.
    /// This is unbounded and is 0 when fewer than two deltas exist or the mean
    /// delta is zero.
    pub frame_delta_cv: f32,
    /// Mean fraction of horizontal and vertical pixel-neighbour pairs whose
    /// composite colours differ.
    pub mean_edge_density: f32,
    /// Mean excess 16x8 regional occupancy contrast above the exact
    /// same-mass shuffled-mask expectation. Zero identifies spatially uniform
    /// or shuffle-like frames; one identifies maximally region-ordered frames.
    pub ordered_spatial_structure: f32,
    /// Geometric mean of same-mass-shuffle-corrected spatial order at the
    /// fixed 8×4 and 4×2 regional scales. Zero identifies spatially uniform
    /// mass at either scale; one identifies maximally separated object and
    /// negative-space allocation at both scales.
    pub object_scale_composition: f32,
    /// Probability that two detail-bearing 16×8 tiles contain exactly the
    /// same collision-free composite-colour pattern. Solid/blank tiles are
    /// excluded; zero means no repeated detail and one means every detailed
    /// tile is identical.
    pub repetitive_texture: f32,
    /// Mean prominence of a non-zero horizontal or vertical occupancy
    /// autocorrelation peak, sampled every eighth logical frame. Periodic
    /// stripes and phase-shifted wallpaper approach one; compact forms whose
    /// correlation decays smoothly approach zero.
    pub spatial_repeat_autocorrelation: f32,
    /// Persistent foreground mass carried by non-trivial four-connected
    /// components after a 75% occupancy threshold, weighted across logical
    /// resolutions. Blank, transient, and full-screen fields score zero.
    pub persistent_component_structure: f32,
    /// Balance between persistent foreground and its largest connected
    /// four-neighbour background region. Blank/full screens and grids that
    /// fragment negative space score zero; forms in one coherent void score
    /// highly.
    pub connected_negative_space: f32,
    /// Fraction of an 8×4 display grid that was lit at least once.
    pub active_region_fraction: f32,
    /// Mean fraction of the 8×4 grid containing a changed pixel between
    /// consecutive frames.
    pub mean_change_region_fraction: f32,
    /// Strongest active framebuffer recurrence period in frames. Zero means
    /// no sufficiently recurrent loop was found.
    pub loop_period_frames: u32,
    /// Recurrence match ratio multiplied by display-activity gates.
    /// Static screens and one/two-frame flicker score zero.
    pub loop_periodicity: f32,
    /// Number of `00E0` clear-screen instructions executed.
    pub clear_count: u32,
    /// Number of delay-timer set instructions executed.
    pub delay_timer_set_count: u32,
    /// Number of delay-timer sets whose value was non-zero.
    pub delay_timer_nonzero_count: u32,
    /// Number of sound-timer set instructions executed.
    pub sound_timer_set_count: u32,
    /// Number of sound-timer sets whose value was non-zero.
    pub sound_timer_nonzero_count: u32,
    /// Total number of horizontal or vertical scroll instructions executed.
    pub scroll_count: u32,
    /// Mean logical-pixel occupancy that remains stable across the trajectory,
    /// combining low- and high-resolution summaries by their frame counts.
    pub composition_stable_foreground: f32,
    /// Mean logical-pixel activity across valid same-resolution transitions,
    /// combining low- and high-resolution summaries by their frame counts.
    pub composition_active_fraction: f32,
    /// Robust topology score for active logical-frame transitions. This is the
    /// geometric mean of non-singleton, largest-component, adjacency-surplus,
    /// and two-axis effective-support terms.
    pub coherent_change_topology: f32,
    /// Conservative consecutive-transition overlap after exact toggle-back
    /// suppression.
    pub temporal_overlap_reversal: f32,
}

const REGION_COLS: usize = 8;
const REGION_ROWS: usize = 4;
const REGION_COUNT: usize = REGION_COLS * REGION_ROWS;
const REGION_W: usize = HIRES_W / REGION_COLS;
const REGION_H: usize = HIRES_H / REGION_ROWS;
const STRUCTURE_REGION_COLS: usize = 16;
const STRUCTURE_REGION_ROWS: usize = 8;
const STRUCTURE_REGION_COUNT: usize = STRUCTURE_REGION_COLS * STRUCTURE_REGION_ROWS;
const TEXTURE_PATTERN_TABLE_CAPACITY: usize = 256;
const DISPLAY_EDGE_COUNT: usize = HIRES_H * (HIRES_W - 1) + HIRES_W * (HIRES_H - 1);
const LOOP_ANALYSIS_FRAMES: usize = 1_800;
const MAX_LOOP_PERIOD_FRAMES: usize = 600;
const MIN_LOOP_PERIOD_FRAMES: usize = 3;
const MIN_LOOP_MATCH: f32 = 0.75;
const LOOP_PERIOD_SAMPLES: usize = 64;
const LOOP_CANDIDATES_TO_VERIFY: usize = 16;
const MOTION_SPREAD_WINDOWS: usize = 12;
const CHANGED_PIXEL_CAP: usize = 48;
const BROAD_TRANSITION_MIN_ROWS: u32 = 6;
const BROAD_TRANSITION_MIN_COLS: u32 = 12;

/// Frame-derived aggregates kept by [`crate::Engine`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FrameAggregate {
    pub mean_lit_fraction: f32,
    pub peak_lit_fraction: f32,
    pub mean_frame_delta: f32,
    pub mean_capped_changed_pixels: f32,
    pub broad_transition_share: f32,
    pub motion_spread: f32,
    pub motion_spatial_spread: f32,
    pub motion_axis_balance: f32,
    pub frame_delta_cv: f32,
    pub mean_edge_density: f32,
    pub ordered_spatial_structure: f32,
    pub object_scale_composition: f32,
    pub repetitive_texture: f32,
    pub spatial_repeat_autocorrelation: f32,
    pub persistent_component_structure: f32,
    pub connected_negative_space: f32,
    pub active_region_fraction: f32,
    pub mean_change_region_fraction: f32,
    pub composition_stable_foreground: f32,
    pub composition_active_fraction: f32,
    pub coherent_change_topology: f32,
    pub temporal_overlap_reversal: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogicalResolution {
    Low,
    High,
}

impl LogicalResolution {
    const fn index(self) -> usize {
        match self {
            Self::Low => 0,
            Self::High => 1,
        }
    }

    const fn width(self) -> usize {
        match self {
            Self::Low => LORES_W,
            Self::High => HIRES_W,
        }
    }

    const fn height(self) -> usize {
        match self {
            Self::Low => LORES_H,
            Self::High => HIRES_H,
        }
    }

    const fn pixel_count(self) -> usize {
        self.width() * self.height()
    }
}

#[derive(Default)]
struct ResolutionComposition {
    frame_count: u64,
    valid_transition_count: u64,
    occupied_counts: Vec<u64>,
    activity_counts: Vec<u64>,
}

impl ResolutionComposition {
    fn ensure_size(&mut self, pixel_count: usize) {
        if self.occupied_counts.is_empty() {
            self.occupied_counts.resize(pixel_count, 0);
            self.activity_counts.resize(pixel_count, 0);
        }
        debug_assert_eq!(self.occupied_counts.len(), pixel_count);
        debug_assert_eq!(self.activity_counts.len(), pixel_count);
    }
}

#[derive(Debug, Clone, Copy)]
struct ChangedPixel {
    index: usize,
    before: u8,
}

/// Incremental frame metrics. The historical physical-frame accumulator and
/// SHA-256 path are preserved verbatim. Additive logical-frame state retains
/// per-resolution occupancy/activity counts, reusable transition workspaces,
/// and one scalar sample per active topology or contiguous active-transition
/// pair; complete logical frames are not retained.
pub(crate) struct FrameMetricsAccumulator {
    previous_composite: Box<[u8; BUF_SIZE]>,
    trajectory_hasher: Sha256,
    has_previous: bool,
    frame_count: u64,
    lit_fraction_sum: f64,
    peak_lit_fraction: f32,
    edge_density_sum: f64,
    ordered_spatial_structure_sum: f64,
    object_scale_composition_sum: f64,
    repetitive_texture_sum: f64,
    spatial_repeat_autocorrelation_sum: [f64; 2],
    spatial_repeat_autocorrelation_samples: [u64; 2],
    texture_pattern_keys: Box<[u128]>,
    texture_pattern_counts: Box<[u16]>,
    texture_pattern_epochs: Box<[u32]>,
    texture_pattern_epoch: u32,
    delta_count: u64,
    delta_mean: f64,
    delta_m2: f64,
    capped_changed_pixel_sum: u64,
    active_transition_count: u64,
    broad_transition_count: u64,
    changed_pixel_counts: Vec<u32>,
    changed_region_counts: Vec<[u16; REGION_COUNT]>,
    change_region_fraction_sum: f64,
    active_region_mask: u32,
    previous_logical: Vec<u8>,
    current_logical: Vec<u8>,
    previous_resolution: Option<LogicalResolution>,
    composition: [ResolutionComposition; 2],
    changed_mask: Vec<bool>,
    visited_mask: Vec<bool>,
    component_stack: Vec<usize>,
    current_changed: Vec<ChangedPixel>,
    pending_changed: Vec<ChangedPixel>,
    pending_resolution: Option<LogicalResolution>,
    singleton_mass_fractions: Vec<f64>,
    largest_component_mass_fractions: Vec<f64>,
    adjacency_surplus_per_changed_pixel: Vec<f64>,
    effective_row_support_normalized: Vec<f64>,
    effective_column_support_normalized: Vec<f64>,
    changed_mask_overlap_fractions: Vec<f64>,
    exact_toggle_back_fractions: Vec<f64>,
}

impl Default for FrameMetricsAccumulator {
    fn default() -> Self {
        Self {
            previous_composite: Box::new([0; BUF_SIZE]),
            trajectory_hasher: Sha256::new(),
            has_previous: false,
            frame_count: 0,
            lit_fraction_sum: 0.0,
            peak_lit_fraction: 0.0,
            edge_density_sum: 0.0,
            ordered_spatial_structure_sum: 0.0,
            object_scale_composition_sum: 0.0,
            repetitive_texture_sum: 0.0,
            spatial_repeat_autocorrelation_sum: [0.0; 2],
            spatial_repeat_autocorrelation_samples: [0; 2],
            texture_pattern_keys: vec![0; TEXTURE_PATTERN_TABLE_CAPACITY].into_boxed_slice(),
            texture_pattern_counts: vec![0; TEXTURE_PATTERN_TABLE_CAPACITY].into_boxed_slice(),
            texture_pattern_epochs: vec![0; TEXTURE_PATTERN_TABLE_CAPACITY].into_boxed_slice(),
            texture_pattern_epoch: 0,
            delta_count: 0,
            delta_mean: 0.0,
            delta_m2: 0.0,
            capped_changed_pixel_sum: 0,
            active_transition_count: 0,
            broad_transition_count: 0,
            changed_pixel_counts: Vec::new(),
            changed_region_counts: Vec::new(),
            change_region_fraction_sum: 0.0,
            active_region_mask: 0,
            previous_logical: Vec::new(),
            current_logical: Vec::new(),
            previous_resolution: None,
            composition: std::array::from_fn(|_| ResolutionComposition::default()),
            changed_mask: Vec::new(),
            visited_mask: Vec::new(),
            component_stack: Vec::new(),
            current_changed: Vec::new(),
            pending_changed: Vec::new(),
            pending_resolution: None,
            singleton_mass_fractions: Vec::new(),
            largest_component_mass_fractions: Vec::new(),
            adjacency_surplus_per_changed_pixel: Vec::new(),
            effective_row_support_normalized: Vec::new(),
            effective_column_support_normalized: Vec::new(),
            changed_mask_overlap_fractions: Vec::new(),
            exact_toggle_back_fractions: Vec::new(),
        }
    }
}

impl FrameMetricsAccumulator {
    /// Record one framebuffer and return its FNV-1a hash. Hashing preserves
    /// the historical plane-0-then-plane-1 byte order.
    pub(crate) fn record_frame(&mut self, planes: &[[u8; BUF_SIZE]; 2], hires: bool) -> u64 {
        let mut hash = 14695981039346656037u64;
        let mut lit_pixels = 0usize;
        let mut changed_pixels = 0usize;
        let mut differing_edges = 0usize;
        let mut changed_region_mask = 0u32;
        let mut changed_row_mask = 0u64;
        let mut changed_column_mask = 0u128;
        let mut changed_region_counts = [0u16; REGION_COUNT];
        let mut left_colour = 0u8;

        for index in 0..BUF_SIZE {
            let plane_0 = planes[0][index];
            hash ^= plane_0 as u64;
            hash = hash.wrapping_mul(1099511628211);

            let colour = (plane_0 & 1) | ((planes[1][index] & 1) << 1);
            let x = index % HIRES_W;
            let y = index / HIRES_W;
            let region = (y / REGION_H) * REGION_COLS + (x / REGION_W);

            if colour != 0 {
                lit_pixels += 1;
                self.active_region_mask |= 1u32 << region;
            }

            if self.has_previous && colour != self.previous_composite[index] {
                changed_pixels += 1;
                changed_region_mask |= 1u32 << region;
                changed_row_mask |= 1u64 << y;
                changed_column_mask |= 1u128 << x;
                changed_region_counts[region] = changed_region_counts[region].saturating_add(1);
            }
            self.previous_composite[index] = colour;

            if x > 0 && colour != left_colour {
                differing_edges += 1;
            }
            if y > 0 {
                let above_index = index - HIRES_W;
                let above_colour =
                    (planes[0][above_index] & 1) | ((planes[1][above_index] & 1) << 1);
                if colour != above_colour {
                    differing_edges += 1;
                }
            }
            left_colour = colour;
        }

        self.trajectory_hasher
            .update(self.previous_composite.as_ref());

        for &plane_1 in &planes[1] {
            hash ^= plane_1 as u64;
            hash = hash.wrapping_mul(1099511628211);
        }

        let lit_fraction = lit_pixels as f32 / BUF_SIZE as f32;
        self.frame_count += 1;
        self.lit_fraction_sum += lit_fraction as f64;
        self.peak_lit_fraction = self.peak_lit_fraction.max(lit_fraction);
        self.edge_density_sum += differing_edges as f64 / DISPLAY_EDGE_COUNT as f64;

        if self.has_previous {
            let delta = changed_pixels as f64 / BUF_SIZE as f64;
            self.delta_count += 1;
            let mean_delta = delta - self.delta_mean;
            self.delta_mean += mean_delta / self.delta_count as f64;
            self.delta_m2 += mean_delta * (delta - self.delta_mean);
            self.capped_changed_pixel_sum += changed_pixels.min(CHANGED_PIXEL_CAP) as u64;
            if changed_pixels > 0 {
                self.active_transition_count += 1;
                if changed_row_mask.count_ones() >= BROAD_TRANSITION_MIN_ROWS
                    && changed_column_mask.count_ones() >= BROAD_TRANSITION_MIN_COLS
                {
                    self.broad_transition_count += 1;
                }
            }
            self.changed_pixel_counts.push(changed_pixels as u32);
            self.changed_region_counts.push(changed_region_counts);
            self.change_region_fraction_sum +=
                changed_region_mask.count_ones() as f64 / REGION_COUNT as f64;
        } else {
            self.has_previous = true;
        }

        self.record_logical_frame(planes, hires);

        hash
    }

    fn record_logical_frame(&mut self, planes: &[[u8; BUF_SIZE]; 2], hires: bool) {
        let resolution = if hires {
            LogicalResolution::High
        } else {
            LogicalResolution::Low
        };
        let width = resolution.width();
        let height = resolution.height();
        let pixel_count = resolution.pixel_count();
        let region_width = width / STRUCTURE_REGION_COLS;
        let region_height = height / STRUCTURE_REGION_ROWS;
        let mut structure_region_counts = [0u16; STRUCTURE_REGION_COUNT];
        let mut texture_patterns = [0u128; STRUCTURE_REGION_COUNT];
        let mut texture_colour_masks = [0u8; STRUCTURE_REGION_COUNT];

        self.current_logical.clear();
        self.current_logical.reserve(pixel_count);
        for y in 0..height {
            for x in 0..width {
                let physical_index = if hires {
                    y * HIRES_W + x
                } else {
                    (y * 2) * HIRES_W + x * 2
                };
                let pixel =
                    (planes[0][physical_index] & 1) | ((planes[1][physical_index] & 1) << 1);
                self.current_logical.push(pixel);
                let region = (y / region_height) * STRUCTURE_REGION_COLS + (x / region_width);
                let local_x = x % region_width;
                let local_y = y % region_height;
                let tile_pixel = local_y * region_width + local_x;
                texture_patterns[region] |= u128::from(pixel) << (tile_pixel * 2);
                texture_colour_masks[region] |= 1u8 << pixel;
                if pixel != 0 {
                    structure_region_counts[region] =
                        structure_region_counts[region].saturating_add(1);
                }
            }
        }
        self.ordered_spatial_structure_sum +=
            ordered_spatial_structure(&structure_region_counts, pixel_count);
        self.object_scale_composition_sum +=
            object_scale_composition(&structure_region_counts, pixel_count);
        self.repetitive_texture_sum +=
            self.repetitive_texture(&texture_patterns, &texture_colour_masks);

        let composition = &mut self.composition[resolution.index()];
        composition.ensure_size(pixel_count);
        composition.frame_count = composition.frame_count.saturating_add(1);
        let sample_spatial_repeat = sample_spatial_repeat_frame(composition.frame_count);
        for (count, &pixel) in composition
            .occupied_counts
            .iter_mut()
            .zip(&self.current_logical)
        {
            *count = count.saturating_add(u64::from(pixel != 0));
        }

        self.current_changed.clear();
        self.changed_mask.clear();
        self.changed_mask.resize(pixel_count, false);
        if self.previous_resolution == Some(resolution) {
            composition.valid_transition_count =
                composition.valid_transition_count.saturating_add(1);
            for (index, (&before, &after)) in self
                .previous_logical
                .iter()
                .zip(&self.current_logical)
                .enumerate()
            {
                if before != after {
                    composition.activity_counts[index] =
                        composition.activity_counts[index].saturating_add(1);
                    self.changed_mask[index] = true;
                    self.current_changed.push(ChangedPixel { index, before });
                }
            }

            if !self.current_changed.is_empty() {
                self.record_transition_topology(resolution);
                self.record_consecutive_transition(resolution);
                std::mem::swap(&mut self.current_changed, &mut self.pending_changed);
                self.pending_resolution = Some(resolution);
            } else {
                self.pending_changed.clear();
                self.pending_resolution = None;
            }
        } else {
            self.pending_changed.clear();
            self.pending_resolution = None;
        }

        if sample_spatial_repeat {
            self.spatial_repeat_autocorrelation_sum[resolution.index()] +=
                spatial_repeat_autocorrelation(&self.current_logical, width, height);
            self.spatial_repeat_autocorrelation_samples[resolution.index()] =
                self.spatial_repeat_autocorrelation_samples[resolution.index()].saturating_add(1);
        }

        std::mem::swap(&mut self.previous_logical, &mut self.current_logical);
        self.previous_resolution = Some(resolution);
    }

    fn repetitive_texture(
        &mut self,
        patterns: &[u128; STRUCTURE_REGION_COUNT],
        colour_masks: &[u8; STRUCTURE_REGION_COUNT],
    ) -> f64 {
        self.texture_pattern_epoch = self.texture_pattern_epoch.wrapping_add(1);
        if self.texture_pattern_epoch == 0 {
            self.texture_pattern_epochs.fill(0);
            self.texture_pattern_epoch = 1;
        }
        let epoch = self.texture_pattern_epoch;
        let mut detail_tile_count = 0u64;
        let mut matching_pairs = 0u64;
        for (&pattern, &colour_mask) in patterns.iter().zip(colour_masks) {
            if colour_mask.count_ones() < 2 {
                continue;
            }
            detail_tile_count += 1;
            let folded = pattern as u64 ^ (pattern >> 64) as u64;
            let mut slot = folded.wrapping_mul(0x9e37_79b9_7f4a_7c15) as usize
                & (TEXTURE_PATTERN_TABLE_CAPACITY - 1);
            loop {
                if self.texture_pattern_epochs[slot] != epoch {
                    self.texture_pattern_epochs[slot] = epoch;
                    self.texture_pattern_keys[slot] = pattern;
                    self.texture_pattern_counts[slot] = 1;
                    break;
                }
                if self.texture_pattern_keys[slot] == pattern {
                    matching_pairs += u64::from(self.texture_pattern_counts[slot]);
                    self.texture_pattern_counts[slot] =
                        self.texture_pattern_counts[slot].saturating_add(1);
                    break;
                }
                slot = (slot + 1) & (TEXTURE_PATTERN_TABLE_CAPACITY - 1);
            }
        }
        if detail_tile_count < 2 {
            0.0
        } else {
            let possible_pairs = detail_tile_count * (detail_tile_count - 1) / 2;
            matching_pairs as f64 / possible_pairs as f64
        }
    }

    fn record_consecutive_transition(&mut self, resolution: LogicalResolution) {
        if self.pending_resolution != Some(resolution) || self.pending_changed.is_empty() {
            return;
        }
        let denominator = self.pending_changed.len().min(self.current_changed.len());
        debug_assert!(denominator > 0);

        let mut left = 0usize;
        let mut right = 0usize;
        let mut overlap = 0usize;
        while left < self.pending_changed.len() && right < self.current_changed.len() {
            match self.pending_changed[left]
                .index
                .cmp(&self.current_changed[right].index)
            {
                std::cmp::Ordering::Less => left += 1,
                std::cmp::Ordering::Greater => right += 1,
                std::cmp::Ordering::Equal => {
                    overlap += 1;
                    left += 1;
                    right += 1;
                }
            }
        }
        let exact_toggle_back = self
            .pending_changed
            .iter()
            .filter(|changed| self.current_logical[changed.index] == changed.before)
            .count();
        self.changed_mask_overlap_fractions
            .push(overlap as f64 / denominator as f64);
        self.exact_toggle_back_fractions
            .push(exact_toggle_back as f64 / denominator as f64);
    }

    fn record_transition_topology(&mut self, resolution: LogicalResolution) {
        let width = resolution.width();
        let height = resolution.height();
        let pixel_count = resolution.pixel_count();
        let changed_mass = self.current_changed.len();

        let mut row_mass = [0usize; HIRES_H];
        let mut column_mass = [0usize; HIRES_W];
        let mut changed_adjacency_pairs = 0usize;
        for changed in &self.current_changed {
            let x = changed.index % width;
            let y = changed.index / width;
            row_mass[y] += 1;
            column_mass[x] += 1;
            if x + 1 < width && self.changed_mask[changed.index + 1] {
                changed_adjacency_pairs += 1;
            }
            if y + 1 < height && self.changed_mask[changed.index + width] {
                changed_adjacency_pairs += 1;
            }
        }

        let row_support = effective_support(&row_mass[..height], changed_mass) / height as f64;
        let column_support = effective_support(&column_mass[..width], changed_mass) / width as f64;
        self.effective_row_support_normalized.push(row_support);
        self.effective_column_support_normalized
            .push(column_support);

        self.visited_mask.clear();
        self.visited_mask.resize(pixel_count, false);
        self.component_stack.clear();
        let mut singleton_count = 0usize;
        let mut largest_component_mass = 0usize;
        for changed in &self.current_changed {
            let start = changed.index;
            if self.visited_mask[start] {
                continue;
            }
            self.visited_mask[start] = true;
            self.component_stack.push(start);
            let mut mass = 0usize;
            while let Some(index) = self.component_stack.pop() {
                mass += 1;
                let x = index % width;
                let y = index / width;
                if x > 0 {
                    visit_changed_neighbor(
                        index - 1,
                        &self.changed_mask,
                        &mut self.visited_mask,
                        &mut self.component_stack,
                    );
                }
                if x + 1 < width {
                    visit_changed_neighbor(
                        index + 1,
                        &self.changed_mask,
                        &mut self.visited_mask,
                        &mut self.component_stack,
                    );
                }
                if y > 0 {
                    visit_changed_neighbor(
                        index - width,
                        &self.changed_mask,
                        &mut self.visited_mask,
                        &mut self.component_stack,
                    );
                }
                if y + 1 < height {
                    visit_changed_neighbor(
                        index + width,
                        &self.changed_mask,
                        &mut self.visited_mask,
                        &mut self.component_stack,
                    );
                }
            }
            singleton_count += usize::from(mass == 1);
            largest_component_mass = largest_component_mass.max(mass);
        }
        self.singleton_mass_fractions
            .push(singleton_count as f64 / changed_mass as f64);
        self.largest_component_mass_fractions
            .push(largest_component_mass as f64 / changed_mass as f64);

        let total_grid_edges = height * width.saturating_sub(1) + width * height.saturating_sub(1);
        let expected_adjacency = if pixel_count > 1 {
            total_grid_edges as f64 * changed_mass as f64 * (changed_mass - 1) as f64
                / (pixel_count as f64 * (pixel_count - 1) as f64)
        } else {
            0.0
        };
        self.adjacency_surplus_per_changed_pixel
            .push((changed_adjacency_pairs as f64 - expected_adjacency) / changed_mass as f64);
    }

    pub(crate) fn trajectory_identity(&self) -> TrajectoryIdentity {
        use std::fmt::Write as _;

        let digest: [u8; 32] = self.trajectory_hasher.clone().finalize().into();
        let mut digest_hex = String::with_capacity(64);
        for byte in digest {
            write!(&mut digest_hex, "{byte:02x}").expect("writing to String cannot fail");
        }
        TrajectoryIdentity {
            definition: TRAJECTORY_IDENTITY_DEFINITION,
            digest: digest_hex,
            frame_count: self.frame_count,
        }
    }

    pub(crate) fn aggregate(&self) -> FrameAggregate {
        let frame_delta_cv = if self.delta_count > 1 && self.delta_mean > f64::EPSILON {
            let sample_variance = self.delta_m2 / (self.delta_count - 1) as f64;
            sample_variance.sqrt() / self.delta_mean
        } else {
            0.0
        };
        let (motion_spatial_spread, motion_axis_balance) =
            motion_spatial_metrics(&self.changed_region_counts);
        let (
            composition_stable_foreground,
            composition_active_fraction,
            persistent_component_structure,
            connected_negative_space,
        ) = composition_metrics(&self.composition);
        let coherent_change_topology = coherent_change_topology(
            &self.singleton_mass_fractions,
            &self.largest_component_mass_fractions,
            &self.adjacency_surplus_per_changed_pixel,
            &self.effective_row_support_normalized,
            &self.effective_column_support_normalized,
        );
        let temporal_overlap_reversal = temporal_overlap_reversal(
            &self.changed_mask_overlap_fractions,
            &self.exact_toggle_back_fractions,
        );

        FrameAggregate {
            mean_lit_fraction: if self.frame_count == 0 {
                0.0
            } else {
                (self.lit_fraction_sum / self.frame_count as f64) as f32
            },
            peak_lit_fraction: self.peak_lit_fraction,
            mean_frame_delta: self.delta_mean as f32,
            mean_capped_changed_pixels: if self.delta_count == 0 {
                0.0
            } else {
                (self.capped_changed_pixel_sum as f64 / self.delta_count as f64) as f32
            },
            broad_transition_share: if self.active_transition_count == 0 {
                0.0
            } else {
                (self.broad_transition_count as f64 / self.active_transition_count as f64) as f32
            },
            motion_spread: motion_spread(&self.changed_pixel_counts),
            motion_spatial_spread,
            motion_axis_balance,
            frame_delta_cv: frame_delta_cv as f32,
            mean_edge_density: if self.frame_count == 0 {
                0.0
            } else {
                (self.edge_density_sum / self.frame_count as f64) as f32
            },
            ordered_spatial_structure: if self.frame_count == 0 {
                0.0
            } else {
                (self.ordered_spatial_structure_sum / self.frame_count as f64) as f32
            },
            object_scale_composition: if self.frame_count == 0 {
                0.0
            } else {
                (self.object_scale_composition_sum / self.frame_count as f64) as f32
            },
            repetitive_texture: if self.frame_count == 0 {
                0.0
            } else {
                (self.repetitive_texture_sum / self.frame_count as f64) as f32
            },
            spatial_repeat_autocorrelation: weighted_spatial_repeat_autocorrelation(
                &self.composition,
                &self.spatial_repeat_autocorrelation_sum,
                &self.spatial_repeat_autocorrelation_samples,
            ),
            persistent_component_structure,
            connected_negative_space,
            active_region_fraction: self.active_region_mask.count_ones() as f32
                / REGION_COUNT as f32,
            mean_change_region_fraction: if self.delta_count == 0 {
                0.0
            } else {
                (self.change_region_fraction_sum / self.delta_count as f64) as f32
            },
            composition_stable_foreground,
            composition_active_fraction,
            coherent_change_topology,
            temporal_overlap_reversal,
        }
    }
}

/// Spatial order above a same-mass shuffled-mask baseline.
///
/// Regional density variance by itself rewards random sampling noise, most
/// strongly on sparse screens. The finite-population hypergeometric term is
/// the exact expected variance when the same lit mass is shuffled uniformly
/// across logical pixels. Removing it makes IID speckle neutral in
/// expectation. Dividing by the remaining Bernoulli ceiling and taking a
/// square root maps the result into `[0, 1]` without fitting corpus labels.
fn ordered_spatial_structure(region_lit_counts: &[u16], pixel_count: usize) -> f64 {
    debug_assert!(!region_lit_counts.is_empty());
    debug_assert_eq!(pixel_count % region_lit_counts.len(), 0);
    let pixels_per_region = pixel_count / region_lit_counts.len();
    let lit_count: usize = region_lit_counts.iter().map(|&count| count as usize).sum();
    if lit_count == 0 || lit_count == pixel_count {
        return 0.0;
    }

    let mean_density = lit_count as f64 / pixel_count as f64;
    let observed_variance = region_lit_counts
        .iter()
        .map(|&count| {
            let density = count as f64 / pixels_per_region as f64;
            (density - mean_density).powi(2)
        })
        .sum::<f64>()
        / region_lit_counts.len() as f64;
    let bernoulli_variance = mean_density * (1.0 - mean_density);
    let shuffled_variance = bernoulli_variance * (pixel_count - pixels_per_region) as f64
        / (pixels_per_region * (pixel_count - 1)) as f64;
    let ordered_variance = (observed_variance - shuffled_variance).max(0.0);
    let ordered_ceiling = (bernoulli_variance - shuffled_variance).max(f64::EPSILON);
    (ordered_variance / ordered_ceiling).clamp(0.0, 1.0).sqrt()
}

/// Same-mass-shuffle-corrected order at the fixed 8×4 and 4×2 scales.
fn object_scale_composition(
    region_lit_counts: &[u16; STRUCTURE_REGION_COUNT],
    pixel_count: usize,
) -> f64 {
    let mut medium_counts = [0u16; REGION_COUNT];
    for medium_y in 0..REGION_ROWS {
        for medium_x in 0..REGION_COLS {
            for dy in 0..2 {
                for dx in 0..2 {
                    medium_counts[medium_y * REGION_COLS + medium_x] += region_lit_counts
                        [(medium_y * 2 + dy) * STRUCTURE_REGION_COLS + medium_x * 2 + dx];
                }
            }
        }
    }
    let mut coarse_counts = [0u16; 8];
    for coarse_y in 0..2 {
        for coarse_x in 0..4 {
            for dy in 0..2 {
                for dx in 0..2 {
                    coarse_counts[coarse_y * 4 + coarse_x] +=
                        medium_counts[(coarse_y * 2 + dy) * REGION_COLS + coarse_x * 2 + dx];
                }
            }
        }
    }
    let medium = ordered_spatial_structure(&medium_counts, pixel_count);
    let coarse = ordered_spatial_structure(&coarse_counts, pixel_count);
    (medium * coarse).sqrt()
}

/// Prominence of periodic secondary peaks in logical occupancy correlation.
///
/// A compact form normally produces an autocorrelation curve that decays from
/// the zero-lag peak. Repeated rails, columns, and phase-shifted wallpaper
/// instead rebound at one or more non-zero lags. Circular comparison makes the
/// diagnostic translation/phase invariant; density centring prevents blank or
/// full frames from scoring.
fn spatial_repeat_autocorrelation(logical: &[u8], width: usize, height: usize) -> f64 {
    debug_assert_eq!(logical.len(), width * height);
    debug_assert!(width <= 128);
    let mut rows = vec![0u128; height];
    let mut lit_count = 0usize;
    for (index, &pixel) in logical.iter().enumerate() {
        if pixel != 0 {
            rows[index / width] |= 1u128 << (index % width);
            lit_count += 1;
        }
    }
    let pixel_count = logical.len();
    if lit_count == 0 || lit_count == pixel_count {
        return 0.0;
    }
    let density = lit_count as f64 / pixel_count as f64;
    let variance = density * (1.0 - density);
    if variance <= f64::EPSILON {
        return 0.0;
    }

    let horizontal = autocorrelation_peak_prominence((1..=width / 2).map(|lag| {
        let joint = rows
            .iter()
            .map(|&row| (row & rotate_logical_row(row, width, lag)).count_ones() as usize)
            .sum::<usize>();
        centred_binary_correlation(joint, pixel_count, density, variance)
    }));
    let vertical = autocorrelation_peak_prominence((1..=height / 2).map(|lag| {
        let joint = rows
            .iter()
            .enumerate()
            .map(|(y, &row)| (row & rows[(y + lag) % height]).count_ones() as usize)
            .sum::<usize>();
        centred_binary_correlation(joint, pixel_count, density, variance)
    }));
    horizontal.max(vertical).clamp(0.0, 1.0)
}

/// Deterministic irregular 1/8 sampling avoids an obvious every-eighth-frame
/// cadence target without retaining a trajectory-sized frame history.
fn sample_spatial_repeat_frame(frame_count: u64) -> bool {
    let mut mixed = frame_count.wrapping_add(0x9e37_79b9_7f4a_7c15);
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (mixed ^ (mixed >> 31)) & 7 == 0
}

fn weighted_spatial_repeat_autocorrelation(
    composition: &[ResolutionComposition; 2],
    sums: &[f64; 2],
    samples: &[u64; 2],
) -> f32 {
    let total_frames: u64 = composition.iter().map(|summary| summary.frame_count).sum();
    if total_frames == 0 {
        return 0.0;
    }
    composition
        .iter()
        .enumerate()
        .filter(|(index, _)| samples[*index] > 0)
        .map(|(index, summary)| {
            let resolution_weight = summary.frame_count as f64 / total_frames as f64;
            resolution_weight * sums[index] / samples[index] as f64
        })
        .sum::<f64>()
        .clamp(0.0, 1.0) as f32
}

fn rotate_logical_row(row: u128, width: usize, lag: usize) -> u128 {
    debug_assert!(width == LORES_W || width == HIRES_W);
    debug_assert!(lag > 0 && lag < width);
    let mask = if width == 128 {
        u128::MAX
    } else {
        (1u128 << width) - 1
    };
    ((row << lag) | (row >> (width - lag))) & mask
}

fn centred_binary_correlation(
    joint_count: usize,
    pixel_count: usize,
    density: f64,
    variance: f64,
) -> f64 {
    let joint_probability = joint_count as f64 / pixel_count as f64;
    ((joint_probability - density * density) / variance).clamp(-1.0, 1.0)
}

fn autocorrelation_peak_prominence(values: impl Iterator<Item = f64>) -> f64 {
    let correlations: Vec<f64> = values.collect();
    if correlations.len() < 3 {
        return 0.0;
    }
    let internal_peak = correlations
        .windows(3)
        .map(|window| (window[1] - 0.5 * (window[0] + window[2])).max(0.0))
        .fold(0.0, f64::max);
    // Circular autocorrelation is symmetric around the half-axis lag, so the
    // omitted neighbour after Nyquist equals the preceding lag.
    let half_axis_peak =
        (correlations[correlations.len() - 1] - correlations[correlations.len() - 2]).max(0.0);
    internal_peak.max(half_axis_peak)
}

fn visit_changed_neighbor(
    index: usize,
    changed_mask: &[bool],
    visited_mask: &mut [bool],
    stack: &mut Vec<usize>,
) {
    if changed_mask[index] && !visited_mask[index] {
        visited_mask[index] = true;
        stack.push(index);
    }
}

fn effective_support(axis_mass: &[usize], total_mass: usize) -> f64 {
    axis_mass
        .iter()
        .copied()
        .filter(|&mass| mass > 0)
        .map(|mass| {
            let probability = mass as f64 / total_mass as f64;
            -probability * probability.log2()
        })
        .sum::<f64>()
        .exp2()
}

fn composition_metrics(composition: &[ResolutionComposition; 2]) -> (f32, f32, f32, f32) {
    let total_frames: u64 = composition.iter().map(|summary| summary.frame_count).sum();
    if total_frames == 0 {
        return (0.0, 0.0, 0.0, 0.0);
    }

    let mut stable_foreground = 0.0f64;
    let mut active_fraction = 0.0f64;
    let mut persistent_component_structure = 0.0f64;
    let mut connected_negative_space = 0.0f64;
    for (summary, resolution) in composition
        .iter()
        .zip([LogicalResolution::Low, LogicalResolution::High])
    {
        if summary.frame_count == 0 {
            continue;
        }
        let pixel_count = summary.occupied_counts.len();
        let mut resolution_stable_foreground = 0.0f64;
        let mut resolution_active_fraction = 0.0f64;
        for (&occupied_count, &activity_count) in
            summary.occupied_counts.iter().zip(&summary.activity_counts)
        {
            let occupancy = occupied_count as f64 / summary.frame_count as f64;
            let activity = if summary.valid_transition_count == 0 {
                0.0
            } else {
                activity_count as f64 / summary.valid_transition_count as f64
            };
            resolution_stable_foreground += occupancy * (1.0 - activity);
            resolution_active_fraction += activity;
        }
        let resolution_weight = summary.frame_count as f64 / total_frames as f64;
        stable_foreground += resolution_weight * resolution_stable_foreground / pixel_count as f64;
        active_fraction += resolution_weight * resolution_active_fraction / pixel_count as f64;
        let (component_structure, negative_space) = persistent_scene_metrics(summary, resolution);
        persistent_component_structure += resolution_weight * component_structure;
        connected_negative_space += resolution_weight * negative_space;
    }
    (
        stable_foreground.clamp(0.0, 1.0) as f32,
        active_fraction.clamp(0.0, 1.0) as f32,
        persistent_component_structure.clamp(0.0, 1.0) as f32,
        connected_negative_space.clamp(0.0, 1.0) as f32,
    )
}

fn persistent_scene_metrics(
    summary: &ResolutionComposition,
    resolution: LogicalResolution,
) -> (f64, f64) {
    let width = resolution.width();
    let height = resolution.height();
    let pixel_count = resolution.pixel_count();
    debug_assert_eq!(summary.occupied_counts.len(), pixel_count);
    let persistent: Vec<bool> = summary
        .occupied_counts
        .iter()
        .map(|&occupied| occupied.saturating_mul(4) >= summary.frame_count.saturating_mul(3))
        .collect();
    let persistent_mass = persistent.iter().filter(|&&value| value).count();
    if persistent_mass == 0 || persistent_mass == pixel_count {
        return (0.0, 0.0);
    }

    let foreground_components = component_sizes(&persistent, true, width, height);
    let minimum_component_mass = pixel_count.div_ceil(512);
    let meaningful: Vec<usize> = foreground_components
        .into_iter()
        .filter(|&mass| mass >= minimum_component_mass)
        .collect();
    let meaningful_mass: usize = meaningful.iter().sum();
    let component_count_score = match meaningful.len() {
        0 => 0.0,
        1 => 0.6,
        2 => 0.8,
        3..=12 => 1.0,
        13..=31 => (32 - meaningful.len()) as f64 / 20.0,
        _ => 0.0,
    };
    let foreground_fraction = persistent_mass as f64 / pixel_count as f64;
    let occupancy_balance = 4.0 * foreground_fraction * (1.0 - foreground_fraction);
    let persistent_component_structure =
        (meaningful_mass as f64 / pixel_count as f64 * occupancy_balance * component_count_score)
            .sqrt();

    let largest_background = component_sizes(&persistent, false, width, height)
        .into_iter()
        .max()
        .unwrap_or(0);
    let background_fraction = largest_background as f64 / pixel_count as f64;
    let connected_negative_space = 2.0 * (background_fraction * foreground_fraction).sqrt();
    (
        persistent_component_structure.clamp(0.0, 1.0),
        connected_negative_space.clamp(0.0, 1.0),
    )
}

fn component_sizes(mask: &[bool], target: bool, width: usize, height: usize) -> Vec<usize> {
    debug_assert_eq!(mask.len(), width * height);
    let mut visited = vec![false; mask.len()];
    let mut stack = Vec::new();
    let mut sizes = Vec::new();
    for start in 0..mask.len() {
        if visited[start] || mask[start] != target {
            continue;
        }
        visited[start] = true;
        stack.push(start);
        let mut size = 0usize;
        while let Some(index) = stack.pop() {
            size += 1;
            let x = index % width;
            let y = index / width;
            if x > 0 {
                visit_component_neighbor(index - 1, mask, target, &mut visited, &mut stack);
            }
            if x + 1 < width {
                visit_component_neighbor(index + 1, mask, target, &mut visited, &mut stack);
            }
            if y > 0 {
                visit_component_neighbor(index - width, mask, target, &mut visited, &mut stack);
            }
            if y + 1 < height {
                visit_component_neighbor(index + width, mask, target, &mut visited, &mut stack);
            }
        }
        sizes.push(size);
    }
    sizes
}

fn visit_component_neighbor(
    index: usize,
    mask: &[bool],
    target: bool,
    visited: &mut [bool],
    stack: &mut Vec<usize>,
) {
    if !visited[index] && mask[index] == target {
        visited[index] = true;
        stack.push(index);
    }
}

fn coherent_change_topology(
    singleton_mass_fractions: &[f64],
    largest_component_mass_fractions: &[f64],
    adjacency_surplus_per_changed_pixel: &[f64],
    effective_row_support_normalized: &[f64],
    effective_column_support_normalized: &[f64],
) -> f32 {
    let Some(singleton_p75) = type_7_quantile(singleton_mass_fractions, 0.75) else {
        return 0.0;
    };
    let largest_p25 = type_7_quantile(largest_component_mass_fractions, 0.25)
        .expect("active transition topology vectors have equal length");
    let adjacency_p25 = type_7_quantile(adjacency_surplus_per_changed_pixel, 0.25)
        .expect("active transition topology vectors have equal length");
    let row_p25 = type_7_quantile(effective_row_support_normalized, 0.25)
        .expect("active transition topology vectors have equal length");
    let column_p25 = type_7_quantile(effective_column_support_normalized, 0.25)
        .expect("active transition topology vectors have equal length");

    let non_singleton = (1.0 - singleton_p75).clamp(0.0, 1.0);
    let largest = largest_p25.clamp(0.0, 1.0);
    let adjacency = (adjacency_p25.max(0.0) / 2.0).clamp(0.0, 1.0);
    let two_axis_support = (row_p25.clamp(0.0, 1.0) * column_p25.clamp(0.0, 1.0)).sqrt();
    (non_singleton * largest * adjacency * two_axis_support)
        .sqrt()
        .sqrt()
        .clamp(0.0, 1.0) as f32
}

fn temporal_overlap_reversal(overlap_fractions: &[f64], toggle_fractions: &[f64]) -> f32 {
    let Some(overlap_p25) = type_7_quantile(overlap_fractions, 0.25) else {
        return 0.0;
    };
    let toggle_p75 = type_7_quantile(toggle_fractions, 0.75)
        .expect("consecutive transition vectors have equal length");
    (overlap_p25 * (1.0 - toggle_p75)).clamp(0.0, 1.0) as f32
}

fn type_7_quantile(values: &[f64], probability: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable_by(f64::total_cmp);
    let h = (sorted.len() - 1) as f64 * probability;
    let lower = h.floor() as usize;
    let upper = h.ceil() as usize;
    let fraction = h - lower as f64;
    Some(sorted[lower] + fraction * (sorted[upper] - sorted[lower]))
}

/// Normalised temporal entropy of changed-pixel energy.
///
/// Transitions are partitioned chronologically into at most 12 bins using
/// `floor(index * bin_count / transition_count)`. The bins therefore differ
/// in transition count by at most one and are all non-empty. Entropy is
/// normalised by the maximum possible entropy for the actual bin count.
fn motion_spread(changed_pixel_counts: &[u32]) -> f32 {
    let transition_count = changed_pixel_counts.len();
    let bin_count = MOTION_SPREAD_WINDOWS.min(transition_count);
    if bin_count < 2 {
        return 0.0;
    }

    let mut bin_energy = [0u64; MOTION_SPREAD_WINDOWS];
    for (index, &changed_pixels) in changed_pixel_counts.iter().enumerate() {
        let bin = index * bin_count / transition_count;
        bin_energy[bin] += u64::from(changed_pixels);
    }

    let total_energy: u64 = bin_energy[..bin_count].iter().sum();
    if total_energy == 0 {
        return 0.0;
    }

    let total = total_energy as f64;
    let entropy = bin_energy[..bin_count]
        .iter()
        .filter(|&&energy| energy > 0)
        .map(|&energy| {
            let probability = energy as f64 / total;
            -probability * probability.log2()
        })
        .sum::<f64>();
    if entropy <= 0.0 {
        return 0.0;
    }
    (entropy / (bin_count as f64).log2()).clamp(0.0, 1.0) as f32
}

/// Mean normalised spatial entropy of changed-pixel energy over the fixed 8×4
/// physical-display grid, evaluated in up to 12 equal timeline windows.
///
/// Transitions use the same chronological bin assignment as [`motion_spread`].
/// Every changed physical pixel contributes one unit to its region. Each
/// window's entropy is evaluated in ascending region-index order and
/// normalised by `log2(32)`, then all actual windows (including zero-energy
/// windows) are averaged.
#[cfg(test)]
fn motion_spatial_spread(changed_region_counts: &[[u16; REGION_COUNT]]) -> f32 {
    motion_spatial_metrics(changed_region_counts).0
}

/// Mean axis bottleneck of changed-pixel energy over the fixed 8×4 grid.
///
/// Each timeline window independently computes normalised entropy across
/// eight column marginals and four row marginals. Taking the weaker axis
/// makes a full-width one-row strip and full-height one-column strip both
/// score zero. The final metric is the arithmetic mean across all actual
/// windows, including zero-energy windows.
#[cfg(test)]
fn motion_axis_balance(changed_region_counts: &[[u16; REGION_COUNT]]) -> f32 {
    motion_spatial_metrics(changed_region_counts).1
}

fn motion_spatial_metrics(changed_region_counts: &[[u16; REGION_COUNT]]) -> (f32, f32) {
    let transition_count = changed_region_counts.len();
    let bin_count = MOTION_SPREAD_WINDOWS.min(transition_count);
    if bin_count == 0 {
        return (0.0, 0.0);
    }

    let mut bin_region_energy = [[0u64; REGION_COUNT]; MOTION_SPREAD_WINDOWS];
    for (index, region_counts) in changed_region_counts.iter().enumerate() {
        let bin = index * bin_count / transition_count;
        for (region, &count) in region_counts.iter().enumerate() {
            bin_region_energy[bin][region] =
                bin_region_energy[bin][region].saturating_add(u64::from(count));
        }
    }

    let spatial_normalisation = (REGION_COUNT as f64).log2();
    let mut spatial_entropy_sum = 0.0;
    let mut axis_balance_sum = 0.0;
    for region_energy in &bin_region_energy[..bin_count] {
        let total_energy: u128 = region_energy.iter().map(|&energy| u128::from(energy)).sum();
        if total_energy == 0 {
            continue;
        }

        let total = total_energy as f64;
        let spatial_entropy = region_energy
            .iter()
            .filter(|&&energy| energy > 0)
            .map(|&energy| {
                let probability = energy as f64 / total;
                -probability * probability.log2()
            })
            .sum::<f64>();
        if spatial_entropy > 0.0 {
            spatial_entropy_sum += (spatial_entropy / spatial_normalisation).clamp(0.0, 1.0);
        }

        let mut column_energy = [0u128; REGION_COLS];
        let mut row_energy = [0u128; REGION_ROWS];
        for (region, &energy) in region_energy.iter().enumerate() {
            column_energy[region % REGION_COLS] += u128::from(energy);
            row_energy[region / REGION_COLS] += u128::from(energy);
        }
        let marginal_entropy = |energy: &[u128], normalisation: f64| {
            let entropy = energy
                .iter()
                .filter(|&&value| value > 0)
                .map(|&value| {
                    let probability = value as f64 / total;
                    -probability * probability.log2()
                })
                .sum::<f64>();
            if entropy <= 0.0 {
                0.0
            } else {
                (entropy / normalisation).clamp(0.0, 1.0)
            }
        };
        let column_entropy = marginal_entropy(&column_energy, (REGION_COLS as f64).log2());
        let row_entropy = marginal_entropy(&row_energy, (REGION_ROWS as f64).log2());
        axis_balance_sum += column_entropy.min(row_entropy);
    }

    let mean_spatial_entropy = spatial_entropy_sum / bin_count as f64;
    let mean_axis_balance = axis_balance_sum / bin_count as f64;
    (
        if mean_spatial_entropy <= 0.0 {
            0.0
        } else {
            mean_spatial_entropy.clamp(0.0, 1.0) as f32
        },
        if mean_axis_balance <= 0.0 {
            0.0
        } else {
            mean_axis_balance.clamp(0.0, 1.0) as f32
        },
    )
}

/// Event-derived aggregates used by both execution and interestingness
/// summaries. Keeping this as one scan avoids repeatedly traversing long logs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct EventCounts {
    pub draw_count: u32,
    pub collision_count: u32,
    pub input_opcode_count: u32,
    pub clear_count: u32,
    pub delay_timer_set_count: u32,
    pub delay_timer_nonzero_count: u32,
    pub sound_timer_set_count: u32,
    pub sound_timer_nonzero_count: u32,
    pub scroll_count: u32,
}

pub(crate) fn count_events(events: &[Event]) -> EventCounts {
    let mut counts = EventCounts::default();
    for event in events {
        match event {
            Event::Draw { collision, .. } => {
                counts.draw_count = counts.draw_count.saturating_add(1);
                if *collision {
                    counts.collision_count = counts.collision_count.saturating_add(1);
                }
            }
            Event::KeyWaitEntered | Event::KeyWaitResolved { .. } => {
                counts.input_opcode_count = counts.input_opcode_count.saturating_add(1);
            }
            Event::ClearScreen => {
                counts.clear_count = counts.clear_count.saturating_add(1);
            }
            Event::TimerSet {
                timer: Timer::Delay,
                value,
            } => {
                counts.delay_timer_set_count = counts.delay_timer_set_count.saturating_add(1);
                if *value != 0 {
                    counts.delay_timer_nonzero_count =
                        counts.delay_timer_nonzero_count.saturating_add(1);
                }
            }
            Event::TimerSet {
                timer: Timer::Sound,
                value,
            } => {
                counts.sound_timer_set_count = counts.sound_timer_set_count.saturating_add(1);
                if *value != 0 {
                    counts.sound_timer_nonzero_count =
                        counts.sound_timer_nonzero_count.saturating_add(1);
                }
            }
            Event::ScrollDown { .. }
            | Event::ScrollUp { .. }
            | Event::ScrollRight
            | Event::ScrollLeft => {
                counts.scroll_count = counts.scroll_count.saturating_add(1);
            }
            _ => {}
        }
    }
    counts
}

// ── Metric Functions ──────────────────────────────────────────────────────────

/// Shannon entropy (bits) of the distribution of `frame_hashes`.
pub fn frame_entropy(frame_hashes: &[u64]) -> f32 {
    if frame_hashes.is_empty() {
        return 0.0;
    }
    let mut counts: std::collections::HashMap<u64, u32> = std::collections::HashMap::new();
    for &h in frame_hashes {
        *counts.entry(h).or_default() += 1;
    }

    let total = frame_hashes.len() as f64;
    let mut contributions: Vec<f64> = counts
        .into_values()
        .map(|count| {
            let probability = count as f64 / total;
            -probability * probability.log2()
        })
        .collect();
    contributions.sort_unstable_by(f64::total_cmp);

    // Kahan summation in a stable order avoids both HashMap iteration
    // nondeterminism and avoidable loss when contributions differ in scale.
    let mut entropy = 0.0f64;
    let mut compensation = 0.0f64;
    for contribution in contributions {
        let adjusted = contribution - compensation;
        let next = entropy + adjusted;
        compensation = (next - entropy) - adjusted;
        entropy = next;
    }
    entropy as f32
}

/// Fraction of consecutive frame-pairs where the hash changed.
pub fn change_rate(frame_hashes: &[u64]) -> f32 {
    if frame_hashes.len() < 2 {
        return 0.0;
    }
    let changes = frame_hashes.windows(2).filter(|w| w[0] != w[1]).count();
    changes as f32 / (frame_hashes.len() - 1) as f32
}

/// Zero-based index of the final frame that differs from its predecessor.
///
/// Zero is returned when the trajectory contains no changed transition.
pub fn last_change_frame(frame_hashes: &[u64]) -> u32 {
    frame_hashes
        .windows(2)
        .enumerate()
        .filter_map(|(transition, pair)| (pair[0] != pair[1]).then_some(transition + 1))
        .next_back()
        .map_or(0, |frame| u32::try_from(frame).unwrap_or(u32::MAX))
}

/// Fraction of changed transitions in the final quarter of the trajectory.
///
/// For `T = frame_hashes.len() - 1` transitions, the measured suffix starts
/// at `floor(3*T/4)` and therefore contains `ceil(T/4)` transitions.
pub fn late_change_rate(frame_hashes: &[u64]) -> f32 {
    let transition_count = frame_hashes.len().saturating_sub(1);
    if transition_count == 0 {
        return 0.0;
    }
    let start = 3 * transition_count / 4;
    let late_transitions = transition_count - start;
    let changes = frame_hashes
        .windows(2)
        .skip(start)
        .filter(|pair| pair[0] != pair[1])
        .count();
    changes as f32 / late_transitions as f32
}

/// Fraction of final-quarter frames that introduce a previously unseen hash.
///
/// The seen set is populated over the full trajectory in chronological order,
/// so a hash first seen before the final quarter is not rediscovered there.
pub fn late_frame_discovery_rate(frame_hashes: &[u64]) -> f32 {
    if frame_hashes.is_empty() {
        return 0.0;
    }
    let start = 3 * frame_hashes.len() / 4;
    let mut seen = HashSet::with_capacity(frame_hashes.len());
    let discoveries = frame_hashes
        .iter()
        .enumerate()
        .filter(|&(frame, hash)| seen.insert(*hash) && frame >= start)
        .count();
    discoveries as f32 / (frame_hashes.len() - start) as f32
}

pub const OPCODE_CLASS_COUNT: usize = 16;

/// Normalised Shannon entropy over a 16-class opcode histogram.
pub fn opcode_diversity(counts: &[u32; OPCODE_CLASS_COUNT]) -> f32 {
    let total: u32 = counts.iter().sum();
    if total == 0 {
        return 0.0;
    }
    let mut entropy = 0.0f32;
    for &c in counts {
        if c > 0 {
            let p = c as f32 / total as f32;
            entropy -= p * p.log2();
        }
    }
    // Normalise by max possible entropy.
    entropy / (OPCODE_CLASS_COUNT as f32).log2()
}

/// `unique_pcs / sqrt(total_cycles)` — how quickly new code is reached.
pub fn coverage_growth(unique_pcs: usize, total_cycles: u64) -> f32 {
    if total_cycles == 0 {
        return 0.0;
    }
    unique_pcs as f32 / (total_cycles as f32).sqrt()
}

/// Fraction of input-event frames where the screen state changed between the
/// preceding and following frames.
pub fn input_responsiveness(input_frames: &[u64], frame_hashes: &[u64]) -> f32 {
    if input_frames.is_empty() || frame_hashes.len() < 2 {
        return 0.0;
    }
    let mut responsive = 0u32;
    for &input_f in input_frames {
        let idx = input_f as usize;
        if idx > 0 && idx + 1 < frame_hashes.len() && frame_hashes[idx + 1] != frame_hashes[idx - 1]
        {
            responsive += 1;
        }
    }
    responsive as f32 / input_frames.len() as f32
}

/// Draw instructions executed per total cycle.
pub fn draw_density(draw_count: u32, total_cycles: u64) -> f32 {
    if total_cycles == 0 {
        return 0.0;
    }
    draw_count as f32 / total_cycles as f32
}

/// Find a strong framebuffer-hash recurrence in a bounded tail window.
///
/// Every candidate period is ranked from 64 evenly spaced comparisons, then
/// the strongest 16 candidates are measured across the full tail. This keeps
/// the search bounded while allowing long demo cycles: at most the final
/// 1,800 frames and periods up to 600 frames are considered. A candidate must
/// fit at least three repeats and match at least 75% of comparable frames.
///
/// The returned strength is the exact-match fraction multiplied by:
/// - a change-rate gate that saturates at 5% changing frame-pairs, and
/// - a changed-pixel gate that saturates at two physical pixels per frame.
///
/// Static output and one/two-frame flicker return `(0, 0.0)`.
pub fn frame_loop_periodicity(frame_hashes: &[u64], mean_frame_delta: f32) -> (u32, f32) {
    let tail_start = frame_hashes.len().saturating_sub(LOOP_ANALYSIS_FRAMES);
    let tail = &frame_hashes[tail_start..];
    let max_period = MAX_LOOP_PERIOD_FRAMES.min(tail.len() / 3);
    if max_period < MIN_LOOP_PERIOD_FRAMES || mean_frame_delta <= 0.0 {
        return (0, 0.0);
    }

    let mut candidates = Vec::with_capacity(max_period);
    for period in 1..=max_period {
        let comparisons = tail.len() - period;
        let sample_count = LOOP_PERIOD_SAMPLES.min(comparisons);
        let matches = (0..sample_count)
            .filter(|&sample| {
                let offset = if sample_count <= 1 {
                    0
                } else {
                    sample * (comparisons - 1) / (sample_count - 1)
                };
                let index = period + offset;
                tail[index] == tail[index - period]
            })
            .count();
        candidates.push((matches as f32 / sample_count as f32, period));
    }
    candidates.sort_unstable_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
    });

    let mut best_period = 0usize;
    let mut best_match = 0.0f32;
    for &(_, period) in candidates.iter().take(LOOP_CANDIDATES_TO_VERIFY) {
        let comparisons = tail.len() - period;
        let matches = (period..tail.len())
            .filter(|&index| tail[index] == tail[index - period])
            .count();
        let recurrence = matches as f32 / comparisons as f32;
        if recurrence > best_match
            || ((recurrence - best_match).abs() <= f32::EPSILON
                && (best_period == 0 || period < best_period))
        {
            best_match = recurrence;
            best_period = period;
        }
    }

    if best_period < MIN_LOOP_PERIOD_FRAMES || best_match < MIN_LOOP_MATCH {
        return (0, 0.0);
    }

    let change_gate = (change_rate(tail) / 0.05).clamp(0.0, 1.0);
    let delta_gate = (mean_frame_delta / (2.0 / BUF_SIZE as f32)).clamp(0.0, 1.0);
    let periodicity = (best_match * change_gate * delta_gate).clamp(0.0, 1.0);
    if periodicity == 0.0 {
        (0, 0.0)
    } else {
        (best_period as u32, periodicity)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn logical_frame(hires: bool, pixels: &[(usize, usize, u8)]) -> [[u8; BUF_SIZE]; 2] {
        let mut frame = [[0u8; BUF_SIZE]; 2];
        for &(x, y, colour) in pixels {
            assert!(colour <= 3);
            let scale = if hires { 1 } else { 2 };
            for dy in 0..scale {
                for dx in 0..scale {
                    let index = (y * scale + dy) * HIRES_W + x * scale + dx;
                    frame[0][index] = colour & 1;
                    frame[1][index] = (colour >> 1) & 1;
                }
            }
        }
        frame
    }

    fn rectangular_frame(
        hires: bool,
        x_start: usize,
        y_start: usize,
        width: usize,
        height: usize,
        colour: u8,
    ) -> [[u8; BUF_SIZE]; 2] {
        let pixels = (y_start..y_start + height)
            .flat_map(|y| (x_start..x_start + width).map(move |x| (x, y, colour)))
            .collect::<Vec<_>>();
        logical_frame(hires, &pixels)
    }

    #[test]
    fn trajectory_identity_of_no_completed_frames_is_sha256_empty() {
        let identity = FrameMetricsAccumulator::default().trajectory_identity();

        assert_eq!(identity.definition, TRAJECTORY_IDENTITY_DEFINITION);
        assert_eq!(
            identity.digest,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(identity.frame_count, 0);
    }

    #[test]
    fn trajectory_identity_matches_known_physical_composite_frames() {
        let mut first = [[0u8; BUF_SIZE]; 2];
        first[0][0] = 1;
        first[1][1] = 1;
        first[0][HIRES_W] = 1;
        first[1][HIRES_W] = 1;

        let mut second = [[0u8; BUF_SIZE]; 2];
        second[1][0] = 1;
        second[0][BUF_SIZE - 1] = 1;

        let mut accumulator = FrameMetricsAccumulator::default();
        accumulator.record_frame(&first, false);
        accumulator.record_frame(&second, false);
        let identity = accumulator.trajectory_identity();

        assert_eq!(
            identity.digest,
            "dbbe3a6426fd85522e59b34660bbaff78c395590b785e9c1b9f08faf94528c79"
        );
        assert_eq!(identity.frame_count, 2);
    }

    #[test]
    fn new_visual_semantics_are_zero_without_frames_or_active_samples() {
        let empty = FrameMetricsAccumulator::default().aggregate();
        assert_eq!(empty.composition_stable_foreground.to_bits(), 0);
        assert_eq!(empty.composition_active_fraction.to_bits(), 0);
        assert_eq!(empty.coherent_change_topology.to_bits(), 0);
        assert_eq!(empty.temporal_overlap_reversal.to_bits(), 0);
        assert_eq!(empty.ordered_spatial_structure.to_bits(), 0);
        assert_eq!(empty.object_scale_composition.to_bits(), 0);
        assert_eq!(empty.repetitive_texture.to_bits(), 0);
        assert_eq!(empty.spatial_repeat_autocorrelation.to_bits(), 0);
        assert_eq!(empty.persistent_component_structure.to_bits(), 0);
        assert_eq!(empty.connected_negative_space.to_bits(), 0);

        let mut static_run = FrameMetricsAccumulator::default();
        let frame = rectangular_frame(false, 0, 0, LORES_W / 2, LORES_H / 2, 1);
        static_run.record_frame(&frame, false);
        static_run.record_frame(&frame, false);
        let summary = static_run.aggregate();
        assert!((summary.composition_stable_foreground - 0.25).abs() < 1e-7);
        assert_eq!(summary.composition_active_fraction.to_bits(), 0);
        assert_eq!(summary.coherent_change_topology.to_bits(), 0);
        assert_eq!(summary.temporal_overlap_reversal.to_bits(), 0);
    }

    #[test]
    fn ordered_spatial_structure_rewards_order_above_same_mass_shuffle() {
        let clustered_pixels = (0..8)
            .flat_map(|y| (0..16).map(move |x| (x, y, 1)))
            .collect::<Vec<_>>();
        let spread_pixels = (0..STRUCTURE_REGION_ROWS)
            .flat_map(|region_y| {
                (0..STRUCTURE_REGION_COLS)
                    .map(move |region_x| (region_x * 4 + 1, region_y * 4 + 1, 1))
            })
            .collect::<Vec<_>>();
        assert_eq!(clustered_pixels.len(), spread_pixels.len());

        let score = |pixels: &[(usize, usize, u8)]| {
            let mut accumulator = FrameMetricsAccumulator::default();
            accumulator.record_frame(&logical_frame(false, pixels), false);
            accumulator.aggregate().ordered_spatial_structure
        };
        let clustered = score(&clustered_pixels);
        let spread = score(&spread_pixels);

        assert!(clustered > 0.95, "clustered score was {clustered}");
        assert_eq!(spread.to_bits(), 0);
    }

    #[test]
    fn ordered_spatial_structure_rejects_uniform_checker_texture() {
        let checker = (0..LORES_H)
            .flat_map(|y| {
                (0..LORES_W)
                    .filter(move |x| (x + y) % 2 == 0)
                    .map(move |x| (x, y, 1))
            })
            .collect::<Vec<_>>();
        let mut accumulator = FrameMetricsAccumulator::default();
        accumulator.record_frame(&logical_frame(false, &checker), false);

        assert_eq!(
            accumulator.aggregate().ordered_spatial_structure.to_bits(),
            0
        );
    }

    #[test]
    fn ordered_spatial_structure_is_resolution_invariant_and_frame_averaged() {
        let low = rectangular_frame(false, 0, 0, LORES_W / 2, LORES_H, 1);
        let high = rectangular_frame(true, 0, 0, HIRES_W / 2, HIRES_H, 1);
        let score = |frame: &[[u8; BUF_SIZE]; 2], hires| {
            let mut accumulator = FrameMetricsAccumulator::default();
            accumulator.record_frame(frame, hires);
            accumulator.aggregate().ordered_spatial_structure
        };

        assert_eq!(score(&low, false).to_bits(), 1.0f32.to_bits());
        assert_eq!(score(&high, true).to_bits(), 1.0f32.to_bits());

        let mut mixed = FrameMetricsAccumulator::default();
        mixed.record_frame(&low, false);
        mixed.record_frame(&logical_frame(true, &[]), true);
        assert_eq!(
            mixed.aggregate().ordered_spatial_structure.to_bits(),
            0.5f32.to_bits()
        );
    }

    #[test]
    fn object_scale_composition_rewards_forms_with_negative_space() {
        let object = rectangular_frame(false, 12, 8, 28, 16, 1);
        let spread_pixels = (0..STRUCTURE_REGION_ROWS)
            .flat_map(|region_y| {
                (0..STRUCTURE_REGION_COLS)
                    .map(move |region_x| (region_x * 4 + 1, region_y * 4 + 1, 1))
            })
            .collect::<Vec<_>>();
        let mut object_accumulator = FrameMetricsAccumulator::default();
        object_accumulator.record_frame(&object, false);
        let mut spread_accumulator = FrameMetricsAccumulator::default();
        spread_accumulator.record_frame(&logical_frame(false, &spread_pixels), false);

        let object_score = object_accumulator.aggregate().object_scale_composition;
        assert!(object_score > 0.60, "object score was {object_score}");
        assert_eq!(
            spread_accumulator
                .aggregate()
                .object_scale_composition
                .to_bits(),
            0
        );
    }

    #[test]
    fn repetitive_texture_detects_periodic_copy_not_one_large_form() {
        let checker = (0..LORES_H)
            .flat_map(|y| {
                (0..LORES_W)
                    .filter(move |x| (x + y) % 2 == 0)
                    .map(move |x| (x, y, 1))
            })
            .collect::<Vec<_>>();
        let mut checker_accumulator = FrameMetricsAccumulator::default();
        checker_accumulator.record_frame(&logical_frame(false, &checker), false);
        let mut object_accumulator = FrameMetricsAccumulator::default();
        object_accumulator.record_frame(
            &rectangular_frame(false, 0, 0, LORES_W / 2, LORES_H, 1),
            false,
        );

        let checker_summary = checker_accumulator.aggregate();
        assert_eq!(checker_summary.object_scale_composition.to_bits(), 0);
        assert_eq!(
            checker_summary.repetitive_texture.to_bits(),
            1.0f32.to_bits()
        );
        assert_eq!(
            object_accumulator.aggregate().repetitive_texture.to_bits(),
            0
        );
    }

    #[test]
    fn repetitive_texture_distinguishes_unique_equal_mass_tiles_and_averages() {
        let mut combinations = (0..16).flat_map(|first| {
            (first + 1..16)
                .flat_map(move |second| (second + 1..16).map(move |third| [first, second, third]))
        });
        let mut unique_pixels = Vec::with_capacity(STRUCTURE_REGION_COUNT * 3);
        for region in 0..STRUCTURE_REGION_COUNT {
            let positions = combinations.next().expect("enough three-pixel patterns");
            let region_x = region % STRUCTURE_REGION_COLS;
            let region_y = region / STRUCTURE_REGION_COLS;
            for position in positions {
                unique_pixels.push((region_x * 4 + position % 4, region_y * 4 + position / 4, 1));
            }
        }
        let unique = logical_frame(false, &unique_pixels);
        let checker_pixels = (0..LORES_H)
            .flat_map(|y| {
                (0..LORES_W)
                    .filter(move |x| (x + y) % 2 == 0)
                    .map(move |x| (x, y, 1))
            })
            .collect::<Vec<_>>();

        let mut unique_accumulator = FrameMetricsAccumulator::default();
        unique_accumulator.record_frame(&unique, false);
        assert_eq!(
            unique_accumulator.aggregate().repetitive_texture.to_bits(),
            0
        );

        let mut mixed_accumulator = FrameMetricsAccumulator::default();
        mixed_accumulator.record_frame(&logical_frame(false, &checker_pixels), false);
        mixed_accumulator.record_frame(&unique, false);
        assert_eq!(
            mixed_accumulator.aggregate().repetitive_texture.to_bits(),
            0.5f32.to_bits()
        );
    }

    #[test]
    fn repetitive_texture_handles_full_width_keys_and_hash_collisions() {
        let mut patterns = [0u128; STRUCTURE_REGION_COUNT];
        for (index, pattern) in patterns.iter_mut().enumerate() {
            // These exact keys differ but fold to the same initial hash slot.
            *pattern = if index % 2 == 0 { 1 } else { 1u128 << 64 };
        }
        let colour_masks = [0b0011u8; STRUCTURE_REGION_COUNT];
        let mut accumulator = FrameMetricsAccumulator::default();
        let measured = accumulator.repetitive_texture(&patterns, &colour_masks);
        let class_size = (STRUCTURE_REGION_COUNT / 2) as u64;
        let matching_pairs = 2 * class_size * (class_size - 1) / 2;
        let possible_pairs =
            STRUCTURE_REGION_COUNT as u64 * (STRUCTURE_REGION_COUNT as u64 - 1) / 2;
        let expected = matching_pairs as f64 / possible_pairs as f64;

        assert_eq!(measured.to_bits(), expected.to_bits());
    }

    #[test]
    fn spatial_repeat_autocorrelation_detects_shifted_periodic_fields_not_compact_forms() {
        let periodic = |phase| {
            let pixels = (0..LORES_H)
                .flat_map(|y| {
                    (0..LORES_W)
                        .filter(move |x| (x + phase) % 4 < 2)
                        .map(move |x| (x, y, 1))
                })
                .collect::<Vec<_>>();
            logical_frame(false, &pixels)
        };
        let score = |frame: [[u8; BUF_SIZE]; 2]| {
            let mut accumulator = FrameMetricsAccumulator::default();
            for _ in 0..8 {
                accumulator.record_frame(&frame, false);
            }
            accumulator.aggregate().spatial_repeat_autocorrelation
        };

        let unshifted = score(periodic(0));
        let shifted = score(periodic(1));
        let compact = score(rectangular_frame(
            false,
            LORES_W / 4,
            0,
            LORES_W / 2,
            LORES_H,
            1,
        ));

        assert!(unshifted > 0.95, "periodic score was {unshifted}");
        assert_eq!(shifted.to_bits(), unshifted.to_bits());
        assert_eq!(compact.to_bits(), 0);
    }

    #[test]
    fn spatial_repeat_autocorrelation_includes_the_half_axis_peak() {
        let mut duplicated_halves = vec![0u8; LORES_W * LORES_H];
        for y in 0..LORES_H {
            duplicated_halves[y * LORES_W] = 1;
            duplicated_halves[y * LORES_W + LORES_W / 2] = 1;
        }

        let measured = spatial_repeat_autocorrelation(&duplicated_halves, LORES_W, LORES_H);
        assert!(measured > 0.95, "half-axis score was {measured}");
    }

    #[test]
    fn spatial_repeat_sampling_is_sparse_and_not_fixed_cadence() {
        let samples = (1..=256)
            .filter(|&frame| sample_spatial_repeat_frame(frame))
            .collect::<Vec<_>>();
        assert!(samples.len() >= 20 && samples.len() <= 44, "{samples:?}");
        let gaps = samples
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .collect::<Vec<_>>();
        assert!(gaps.windows(2).any(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn persistent_scene_metrics_reward_a_form_in_open_space_not_wallpaper() {
        let object = rectangular_frame(false, 16, 8, 32, 16, 1);
        let checker_pixels = (0..LORES_H)
            .flat_map(|y| {
                (0..LORES_W)
                    .filter(move |x| (x + y) % 2 == 0)
                    .map(move |x| (x, y, 1))
            })
            .collect::<Vec<_>>();
        let checker = logical_frame(false, &checker_pixels);
        let score = |frame: &[[u8; BUF_SIZE]; 2]| {
            let mut accumulator = FrameMetricsAccumulator::default();
            for _ in 0..8 {
                accumulator.record_frame(frame, false);
            }
            accumulator.aggregate()
        };

        let object = score(&object);
        let checker = score(&checker);
        assert!(
            object.persistent_component_structure > 0.30,
            "object component score was {}",
            object.persistent_component_structure
        );
        assert!(
            object.connected_negative_space > 0.80,
            "object negative-space score was {}",
            object.connected_negative_space
        );
        assert_eq!(checker.persistent_component_structure.to_bits(), 0);
        assert!(
            checker.connected_negative_space < 0.05,
            "checker negative-space score was {}",
            checker.connected_negative_space
        );
    }

    #[test]
    fn persistent_scene_metrics_reject_blank_full_and_transient_masks() {
        let blank = logical_frame(false, &[]);
        let full = rectangular_frame(false, 0, 0, LORES_W, LORES_H, 1);
        let object = rectangular_frame(false, 16, 8, 32, 16, 1);
        let score = |frames: &[&[[u8; BUF_SIZE]; 2]]| {
            let mut accumulator = FrameMetricsAccumulator::default();
            for frame in frames {
                accumulator.record_frame(frame, false);
            }
            accumulator.aggregate()
        };

        for summary in [score(&[&blank; 8]), score(&[&full; 8])] {
            assert_eq!(summary.persistent_component_structure.to_bits(), 0);
            assert_eq!(summary.connected_negative_space.to_bits(), 0);
        }
        let transient = score(&[
            &object, &object, &object, &object, &object, &blank, &blank, &blank,
        ]);
        assert_eq!(transient.persistent_component_structure.to_bits(), 0);
        assert_eq!(transient.connected_negative_space.to_bits(), 0);
        let threshold = score(&[
            &object, &object, &object, &object, &object, &object, &blank, &blank,
        ]);
        assert!(threshold.persistent_component_structure > 0.0);
        assert!(threshold.connected_negative_space > 0.0);
    }

    #[test]
    fn persistent_scene_metrics_weight_logical_resolutions_by_frame_count() {
        let low_object = rectangular_frame(false, 16, 8, 32, 16, 1);
        let high_blank = logical_frame(true, &[]);
        let mut low = FrameMetricsAccumulator::default();
        let mut mixed = FrameMetricsAccumulator::default();
        for _ in 0..8 {
            low.record_frame(&low_object, false);
            mixed.record_frame(&low_object, false);
            mixed.record_frame(&high_blank, true);
        }
        let low = low.aggregate();
        let mixed = mixed.aggregate();

        assert!(
            (mixed.persistent_component_structure - 0.5 * low.persistent_component_structure).abs()
                < 1e-7
        );
        assert!((mixed.connected_negative_space - 0.5 * low.connected_negative_space).abs() < 1e-7);
    }

    #[test]
    fn composition_activity_is_known_logical_pixel_fraction() {
        let mut accumulator = FrameMetricsAccumulator::default();
        accumulator.record_frame(&logical_frame(false, &[]), false);
        accumulator.record_frame(&logical_frame(false, &[(7, 9, 1)]), false);

        let expected = 1.0 / (LORES_W * LORES_H) as f32;
        let actual = accumulator.aggregate().composition_active_fraction;
        assert!((actual - expected).abs() < 1e-9, "activity was {actual}");
    }

    #[test]
    fn coherent_topology_rewards_blocks_and_suppresses_speckles_and_one_axis() {
        let score = |frame| {
            let mut accumulator = FrameMetricsAccumulator::default();
            accumulator.record_frame(&logical_frame(false, &[]), false);
            accumulator.record_frame(&frame, false);
            accumulator.aggregate().coherent_change_topology
        };

        let block = score(rectangular_frame(false, 8, 8, 16, 16, 1));
        let row = score(rectangular_frame(false, 0, 8, LORES_W, 1, 1));
        let speckles = score(logical_frame(
            false,
            &[(1, 1, 1), (10, 4, 1), (22, 9, 1), (35, 14, 1)],
        ));
        let singleton = score(logical_frame(false, &[(5, 5, 1)]));

        assert!(
            block > row,
            "block {block} should exceed one-axis row {row}"
        );
        assert_eq!(speckles.to_bits(), 0);
        assert_eq!(singleton.to_bits(), 0);
    }

    #[test]
    fn temporal_overlap_reversal_rejects_full_toggle_back() {
        let middle = rectangular_frame(false, 8, 8, 8, 8, 1);

        let mut toggle = FrameMetricsAccumulator::default();
        toggle.record_frame(&logical_frame(false, &[]), false);
        toggle.record_frame(&middle, false);
        toggle.record_frame(&logical_frame(false, &[]), false);
        assert_eq!(toggle.aggregate().temporal_overlap_reversal.to_bits(), 0);

        let mut nonreversing = FrameMetricsAccumulator::default();
        nonreversing.record_frame(&logical_frame(false, &[]), false);
        nonreversing.record_frame(&middle, false);
        nonreversing.record_frame(&rectangular_frame(false, 8, 8, 8, 8, 2), false);
        assert_eq!(
            nonreversing.aggregate().temporal_overlap_reversal.to_bits(),
            1.0f32.to_bits()
        );
    }

    #[test]
    fn resolution_changes_are_excluded_and_composition_is_frame_weighted() {
        let low_lit = rectangular_frame(false, 0, 0, LORES_W, LORES_H, 1);
        let high_blank = logical_frame(true, &[]);
        let mut accumulator = FrameMetricsAccumulator::default();
        accumulator.record_frame(&low_lit, false);
        accumulator.record_frame(&low_lit, false);
        accumulator.record_frame(&high_blank, true);

        let summary = accumulator.aggregate();
        assert!((summary.composition_stable_foreground - 2.0 / 3.0).abs() < 1e-7);
        assert_eq!(summary.composition_active_fraction.to_bits(), 0);
        assert_eq!(summary.coherent_change_topology.to_bits(), 0);
        assert_eq!(summary.temporal_overlap_reversal.to_bits(), 0);
        assert_eq!(accumulator.composition[0].valid_transition_count, 1);
        assert_eq!(accumulator.composition[1].valid_transition_count, 0);
    }

    #[test]
    fn type_7_quantiles_interpolate_linearly() {
        assert_eq!(type_7_quantile(&[], 0.25), None);
        assert_eq!(type_7_quantile(&[30.0, 0.0, 20.0, 10.0], 0.25), Some(7.5));
        assert_eq!(type_7_quantile(&[30.0, 0.0, 20.0, 10.0], 0.75), Some(22.5));
    }

    #[test]
    fn resolution_annotation_does_not_change_old_physical_metrics_or_identity() {
        let first = rectangular_frame(false, 2, 3, 5, 4, 1);
        let second = rectangular_frame(false, 4, 5, 9, 7, 2);
        let evaluate = |hires| {
            let mut accumulator = FrameMetricsAccumulator::default();
            accumulator.record_frame(&first, hires);
            accumulator.record_frame(&second, hires);
            let aggregate = accumulator.aggregate();
            let old_metrics = [
                aggregate.mean_lit_fraction.to_bits(),
                aggregate.peak_lit_fraction.to_bits(),
                aggregate.mean_frame_delta.to_bits(),
                aggregate.mean_capped_changed_pixels.to_bits(),
                aggregate.broad_transition_share.to_bits(),
                aggregate.motion_spread.to_bits(),
                aggregate.motion_spatial_spread.to_bits(),
                aggregate.motion_axis_balance.to_bits(),
                aggregate.frame_delta_cv.to_bits(),
                aggregate.mean_edge_density.to_bits(),
                aggregate.active_region_fraction.to_bits(),
                aggregate.mean_change_region_fraction.to_bits(),
            ];
            (old_metrics, accumulator.trajectory_identity())
        };

        assert_eq!(evaluate(false), evaluate(true));
    }

    fn reference_motion_spatial_spread(changed_region_counts: &[[u16; REGION_COUNT]]) -> f32 {
        let transition_count = changed_region_counts.len();
        let bin_count = MOTION_SPREAD_WINDOWS.min(transition_count);
        if bin_count == 0 {
            return 0.0;
        }

        let mut bin_region_energy = [[0u64; REGION_COUNT]; MOTION_SPREAD_WINDOWS];
        for (index, region_counts) in changed_region_counts.iter().enumerate() {
            let bin = index * bin_count / transition_count;
            for (region, &count) in region_counts.iter().enumerate() {
                bin_region_energy[bin][region] =
                    bin_region_energy[bin][region].saturating_add(u64::from(count));
            }
        }

        let normalisation = (REGION_COUNT as f64).log2();
        let entropy_sum = bin_region_energy[..bin_count]
            .iter()
            .map(|region_energy| {
                let total_energy: u128 =
                    region_energy.iter().map(|&energy| u128::from(energy)).sum();
                if total_energy == 0 {
                    return 0.0;
                }

                let total = total_energy as f64;
                let entropy = region_energy
                    .iter()
                    .filter(|&&energy| energy > 0)
                    .map(|&energy| {
                        let probability = energy as f64 / total;
                        -probability * probability.log2()
                    })
                    .sum::<f64>();
                if entropy <= 0.0 {
                    0.0
                } else {
                    (entropy / normalisation).clamp(0.0, 1.0)
                }
            })
            .sum::<f64>();
        let mean_entropy = entropy_sum / bin_count as f64;
        if mean_entropy <= 0.0 {
            return 0.0;
        }
        mean_entropy.clamp(0.0, 1.0) as f32
    }

    #[test]
    fn frame_entropy_all_same() {
        let hashes = vec![42u64; 100];
        assert_eq!(frame_entropy(&hashes), 0.0);
    }

    #[test]
    fn frame_entropy_all_different() {
        let hashes: Vec<u64> = (0..8).collect();
        let e = frame_entropy(&hashes);
        // All unique → maximum entropy = log2(8) = 3.0 bits.
        assert!((e - 3.0).abs() < 1e-5, "entropy was {e}");
    }

    #[test]
    fn frame_entropy_is_bitwise_stable_for_equivalent_histograms() {
        let frequencies: Vec<usize> = (0..256).map(|index| (index * 37) % 251 + 1).collect();
        let build_hashes = |salt: u32| {
            let mut hashes = Vec::new();
            for (index, &frequency) in frequencies.iter().enumerate() {
                let hash = (index as u64)
                    .wrapping_mul(0x9e37_79b9_7f4a_7c15)
                    .wrapping_add(salt as u64)
                    .rotate_left(salt);
                hashes.extend(std::iter::repeat_n(hash, frequency));
            }
            if salt & 1 == 0 {
                hashes.reverse();
            }
            hashes
        };

        let expected = frame_entropy(&build_hashes(0)).to_bits();
        for salt in 1..=32 {
            assert_eq!(
                frame_entropy(&build_hashes(salt)).to_bits(),
                expected,
                "entropy changed for equivalent histogram with salt {salt}",
            );
        }
    }

    #[test]
    fn motion_spread_rejects_front_loaded_construction() {
        let mut changed_pixels = vec![0; 120];
        changed_pixels[..10].fill(100);
        assert_eq!(motion_spread(&changed_pixels).to_bits(), 0.0f32.to_bits());
    }

    #[test]
    fn motion_spread_rewards_uniform_sustained_energy() {
        let changed_pixels = vec![10; 120];
        let spread = motion_spread(&changed_pixels);
        assert!((spread - 1.0).abs() < 1e-6, "spread was {spread}");
    }

    #[test]
    fn motion_spread_tiny_late_change_does_not_rescue_early_burst() {
        let mut changed_pixels = vec![0; 120];
        changed_pixels[..10].fill(100);
        changed_pixels[119] = 1;
        let spread = motion_spread(&changed_pixels);
        assert!(spread > 0.0 && spread < 0.01, "spread was {spread}");
    }

    #[test]
    fn motion_spread_tracks_intermittent_energy_across_windows() {
        let mut changed_pixels = vec![0; 120];
        for index in [5, 35, 65, 95] {
            changed_pixels[index] = 250;
        }
        let spread = motion_spread(&changed_pixels);
        let expected = 4.0f32.log2() / 12.0f32.log2();
        assert!(
            (spread - expected).abs() < 1e-6,
            "spread was {spread}, expected {expected}"
        );
    }

    #[test]
    fn motion_spread_is_deterministic_and_handles_empty_energy() {
        assert_eq!(motion_spread(&[]), 0.0);
        assert_eq!(motion_spread(&[100]), 0.0);
        assert_eq!(motion_spread(&[0; 24]), 0.0);

        let changed_pixels: Vec<u32> = (0..137)
            .map(|index| ((index * 17 + 3) % 23) as u32)
            .collect();
        let expected = motion_spread(&changed_pixels).to_bits();
        for _ in 0..128 {
            assert_eq!(motion_spread(&changed_pixels).to_bits(), expected);
        }
    }

    #[test]
    fn motion_spatial_spread_zero_and_one_region_are_positive_zero() {
        let empty_accumulator = FrameMetricsAccumulator::default();
        assert_eq!(
            empty_accumulator
                .aggregate()
                .motion_spatial_spread
                .to_bits(),
            0.0f32.to_bits()
        );
        assert_eq!(motion_spatial_spread(&[]).to_bits(), 0.0f32.to_bits());

        let zero_energy = vec![[0u16; REGION_COUNT]; 24];
        assert_eq!(
            motion_spatial_spread(&zero_energy).to_bits(),
            0.0f32.to_bits()
        );

        let mut one_region = vec![[0u16; REGION_COUNT]; 24];
        for (index, counts) in one_region.iter_mut().enumerate() {
            counts[17] = (index + 1) as u16;
        }
        assert_eq!(
            motion_spatial_spread(&one_region).to_bits(),
            0.0f32.to_bits()
        );
    }

    #[test]
    fn motion_spatial_spread_uses_fixed_full_grid_normalisation() {
        for populated_regions in [2usize, 4, 8, 32] {
            let mut transition = [0u16; REGION_COUNT];
            transition[..populated_regions].fill(37);
            let changed_regions = vec![transition; 24];
            let spread = motion_spatial_spread(&changed_regions);
            let expected = (populated_regions as f64).log2() / (REGION_COUNT as f64).log2();
            assert!(
                (f64::from(spread) - expected).abs() < 1e-7,
                "{populated_regions} uniform regions produced {spread}, expected {expected}"
            );
        }
    }

    #[test]
    fn motion_spatial_spread_penalises_dominance_and_tracks_uneven_energy() {
        let mut dominant = vec![[0u16; REGION_COUNT]; 1_200];
        for (index, transition) in dominant.iter_mut().enumerate() {
            transition[0] = 256;
            if index % 100 == 0 {
                transition[1..].fill(1);
            }
        }
        let dominant_spread = motion_spatial_spread(&dominant);
        assert!(
            dominant_spread > 0.0 && dominant_spread < 0.005,
            "dominant spread was {dominant_spread}"
        );

        let mut uneven_transition = [0u16; REGION_COUNT];
        uneven_transition[..4].copy_from_slice(&[1, 2, 4, 8]);
        let uneven_spread = motion_spatial_spread(&vec![uneven_transition; 24]);
        assert!(
            (uneven_spread - 0.328_044_77).abs() < 1e-7,
            "uneven spread was {uneven_spread}"
        );
        assert!(uneven_spread > dominant_spread);
    }

    #[test]
    fn motion_spatial_spread_rejects_broad_setup_then_local_blinking() {
        let mut changed_regions = vec![[0u16; REGION_COUNT]; 120];
        for transition in &mut changed_regions[..10] {
            transition.fill(1);
        }
        for transition in &mut changed_regions[10..] {
            transition[0] = 1;
        }

        let spread = motion_spatial_spread(&changed_regions);
        let expected = 1.0 / MOTION_SPREAD_WINDOWS as f32;
        assert!(
            (spread - expected).abs() < 1e-7,
            "setup-then-blink spread was {spread}, expected {expected}"
        );
    }

    #[test]
    fn motion_spatial_spread_counts_empty_timeline_windows() {
        let mut changed_regions = vec![[0u16; REGION_COUNT]; 12];
        changed_regions[0].fill(1);
        let spread = motion_spatial_spread(&changed_regions);
        let expected = 1.0 / MOTION_SPREAD_WINDOWS as f32;
        assert!(
            (spread - expected).abs() < 1e-7,
            "one active window produced {spread}, expected {expected}"
        );
    }

    #[test]
    fn motion_spatial_spread_is_bitwise_stable_for_uneven_energy() {
        let changed_regions: Vec<[u16; REGION_COUNT]> = (0..137)
            .map(|transition| {
                std::array::from_fn(|region| ((transition * 37 + region * 19 + 3) % 257) as u16)
            })
            .collect();
        let expected = motion_spatial_spread(&changed_regions).to_bits();
        for _ in 0..128 {
            assert_eq!(motion_spatial_spread(&changed_regions).to_bits(), expected);
        }
    }

    #[test]
    fn shared_spatial_binning_preserves_previous_metric_bitwise() {
        for transition_count in [0, 1, 2, 11, 12, 13, 137, 1_799] {
            for salt in 0..32 {
                let changed_regions: Vec<[u16; REGION_COUNT]> = (0..transition_count)
                    .map(|transition| {
                        std::array::from_fn(|region| {
                            ((transition * 41 + region * 23 + salt * 17) % 257) as u16
                        })
                    })
                    .collect();
                assert_eq!(
                    motion_spatial_metrics(&changed_regions).0.to_bits(),
                    reference_motion_spatial_spread(&changed_regions).to_bits(),
                    "transition_count={transition_count}, salt={salt}"
                );
            }
        }
    }

    #[test]
    fn motion_axis_balance_rejects_single_row_and_column_strips() {
        let mut row_strip = [0u16; REGION_COUNT];
        row_strip[..REGION_COLS].fill(37);
        let row_transitions = vec![row_strip; 24];
        assert_eq!(
            motion_axis_balance(&row_transitions).to_bits(),
            0.0f32.to_bits()
        );
        assert!((motion_spatial_spread(&row_transitions) - 0.6).abs() < 1e-7);

        let mut column_strip = [0u16; REGION_COUNT];
        for row in 0..REGION_ROWS {
            column_strip[row * REGION_COLS] = 37;
        }
        let column_transitions = vec![column_strip; 24];
        assert_eq!(
            motion_axis_balance(&column_transitions).to_bits(),
            0.0f32.to_bits()
        );
        assert!((motion_spatial_spread(&column_transitions) - 0.4).abs() < 1e-7);
    }

    #[test]
    fn motion_axis_balance_uses_weaker_normalised_marginal() {
        let mut block = [0u16; REGION_COUNT];
        for region in [0, 1, REGION_COLS, REGION_COLS + 1] {
            block[region] = 19;
        }
        let changed_regions = vec![block; 24];
        let expected = 1.0f32 / 3.0;
        let balance = motion_axis_balance(&changed_regions);
        assert!(
            (balance - expected).abs() < 1e-7,
            "2x2 block produced {balance}, expected {expected}"
        );

        let full_grid = vec![[1u16; REGION_COUNT]; 24];
        assert_eq!(motion_axis_balance(&full_grid).to_bits(), 1.0f32.to_bits());
    }

    #[test]
    fn motion_axis_balance_counts_empty_timeline_windows() {
        let mut changed_regions = vec![[0u16; REGION_COUNT]; 12];
        changed_regions[0].fill(1);
        let balance = motion_axis_balance(&changed_regions);
        let expected = 1.0 / MOTION_SPREAD_WINDOWS as f32;
        assert!(
            (balance - expected).abs() < 1e-7,
            "one active window produced {balance}, expected {expected}"
        );
    }

    #[test]
    fn motion_axis_balance_is_bitwise_stable_and_positive_zero() {
        assert_eq!(motion_axis_balance(&[]).to_bits(), 0.0f32.to_bits());
        assert_eq!(
            motion_axis_balance(&vec![[0u16; REGION_COUNT]; 24]).to_bits(),
            0.0f32.to_bits()
        );

        let changed_regions: Vec<[u16; REGION_COUNT]> = (0..137)
            .map(|transition| {
                std::array::from_fn(|region| ((transition * 41 + region * 23 + 5) % 251) as u16)
            })
            .collect();
        let expected = motion_axis_balance(&changed_regions).to_bits();
        for _ in 0..128 {
            assert_eq!(motion_axis_balance(&changed_regions).to_bits(), expected);
        }
    }

    #[test]
    fn change_rate_no_changes() {
        let hashes = vec![7u64; 10];
        assert_eq!(change_rate(&hashes), 0.0);
    }

    #[test]
    fn change_rate_all_changes() {
        let hashes: Vec<u64> = (0..5).collect();
        assert_eq!(change_rate(&hashes), 1.0);
    }

    #[test]
    fn late_horizon_metrics_cover_empty_single_and_static_trajectories() {
        assert_eq!(last_change_frame(&[]), 0);
        assert_eq!(late_change_rate(&[]).to_bits(), 0.0f32.to_bits());
        assert_eq!(late_frame_discovery_rate(&[]).to_bits(), 0.0f32.to_bits());

        assert_eq!(last_change_frame(&[7]), 0);
        assert_eq!(late_change_rate(&[7]).to_bits(), 0.0f32.to_bits());
        assert_eq!(late_frame_discovery_rate(&[7]).to_bits(), 1.0f32.to_bits());

        let static_hashes = [7; 8];
        assert_eq!(last_change_frame(&static_hashes), 0);
        assert_eq!(late_change_rate(&static_hashes).to_bits(), 0.0f32.to_bits());
        assert_eq!(
            late_frame_discovery_rate(&static_hashes).to_bits(),
            0.0f32.to_bits()
        );
    }

    #[test]
    fn late_horizon_metrics_separate_early_and_sustained_change() {
        let early_only = [0, 1, 2, 2, 2, 2, 2, 2];
        assert_eq!(last_change_frame(&early_only), 2);
        assert_eq!(late_change_rate(&early_only).to_bits(), 0.0f32.to_bits());
        assert_eq!(
            late_frame_discovery_rate(&early_only).to_bits(),
            0.0f32.to_bits()
        );

        let sustained = [0, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(last_change_frame(&sustained), 7);
        assert_eq!(late_change_rate(&sustained).to_bits(), 1.0f32.to_bits());
        assert_eq!(
            late_frame_discovery_rate(&sustained).to_bits(),
            1.0f32.to_bits()
        );

        let late_single_change = [0, 0, 0, 0, 0, 0, 1, 1];
        assert_eq!(last_change_frame(&late_single_change), 6);
        assert_eq!(
            late_change_rate(&late_single_change).to_bits(),
            0.5f32.to_bits()
        );
        assert_eq!(
            late_frame_discovery_rate(&late_single_change).to_bits(),
            0.5f32.to_bits()
        );
    }

    #[test]
    fn late_discovery_rejects_repeated_late_loops() {
        let repeated_late_loop = [0, 1, 2, 3, 0, 1, 0, 1];
        let continuing_discovery = [0, 1, 2, 3, 4, 5, 6, 7];

        assert_eq!(
            late_change_rate(&repeated_late_loop).to_bits(),
            1.0f32.to_bits()
        );
        assert_eq!(
            late_change_rate(&continuing_discovery).to_bits(),
            1.0f32.to_bits()
        );
        assert_eq!(
            late_frame_discovery_rate(&repeated_late_loop).to_bits(),
            0.0f32.to_bits()
        );
        assert_eq!(
            late_frame_discovery_rate(&continuing_discovery).to_bits(),
            1.0f32.to_bits()
        );
    }

    #[test]
    fn late_horizon_quarter_boundaries_use_transitions_and_frames_separately() {
        // Five frames contain four transitions. The final-quarter transition
        // starts at index 3, while final-quarter frames start at index 3 and
        // contain frames 3 and 4.
        let hashes = [0, 0, 0, 1, 1];
        assert_eq!(last_change_frame(&hashes), 3);
        assert_eq!(late_change_rate(&hashes).to_bits(), 0.0f32.to_bits());
        assert_eq!(
            late_frame_discovery_rate(&hashes).to_bits(),
            0.5f32.to_bits()
        );

        // Six frames contain five transitions: both final-quarter suffixes
        // contain exactly their last two items.
        let hashes = [0, 0, 0, 0, 1, 2];
        assert_eq!(last_change_frame(&hashes), 5);
        assert_eq!(late_change_rate(&hashes).to_bits(), 1.0f32.to_bits());
        assert_eq!(
            late_frame_discovery_rate(&hashes).to_bits(),
            1.0f32.to_bits()
        );
    }

    #[test]
    fn opcode_diversity_empty() {
        let counts = [0u32; 16];
        assert_eq!(opcode_diversity(&counts), 0.0);
    }

    #[test]
    fn opcode_diversity_uniform() {
        let counts = [1u32; 16];
        let d = opcode_diversity(&counts);
        assert!((d - 1.0).abs() < 1e-5, "diversity was {d}");
    }

    #[test]
    fn draw_density_basic() {
        assert_eq!(draw_density(0, 0), 0.0);
        assert_eq!(draw_density(100, 1000), 0.1);
    }

    #[test]
    fn coverage_growth_basic() {
        assert_eq!(coverage_growth(0, 0), 0.0);
        // 100 PCs in sqrt(10000)=100 cycles → ratio 1.0
        assert!((coverage_growth(100, 10000) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn frame_accumulator_tracks_activity_locality_and_hash_compatibility() {
        let mut accumulator = FrameMetricsAccumulator::default();
        let mut frame = [[0u8; BUF_SIZE]; 2];
        frame[0][0] = 1;

        let first_hash = accumulator.record_frame(&frame, false);
        let mut flattened = Vec::with_capacity(BUF_SIZE * 2);
        flattened.extend_from_slice(&frame[0]);
        flattened.extend_from_slice(&frame[1]);
        assert_eq!(first_hash, crate::emulator::fnv1a_64(&flattened));

        frame[0][0] = 0;
        frame[1][BUF_SIZE - 1] = 1;
        accumulator.record_frame(&frame, false);
        let metrics = accumulator.aggregate();

        let one_pixel = 1.0 / BUF_SIZE as f32;
        assert!((metrics.mean_lit_fraction - one_pixel).abs() < 1e-7);
        assert!((metrics.peak_lit_fraction - one_pixel).abs() < 1e-7);
        assert!((metrics.mean_frame_delta - 2.0 * one_pixel).abs() < 1e-7);
        assert_eq!(metrics.frame_delta_cv, 0.0);
        assert!((metrics.active_region_fraction - 2.0 / REGION_COUNT as f32).abs() < 1e-7);
        assert!((metrics.mean_change_region_fraction - 2.0 / REGION_COUNT as f32).abs() < 1e-7);
        assert!(metrics.mean_edge_density > 0.0);
    }

    #[test]
    fn capped_changed_pixel_mean_is_zero_without_transitions_or_motion() {
        let mut accumulator = FrameMetricsAccumulator::default();
        assert_eq!(
            accumulator.aggregate().mean_capped_changed_pixels.to_bits(),
            0.0f32.to_bits()
        );

        let frame = [[0u8; BUF_SIZE]; 2];
        accumulator.record_frame(&frame, false);
        assert_eq!(
            accumulator.aggregate().mean_capped_changed_pixels.to_bits(),
            0.0f32.to_bits()
        );

        accumulator.record_frame(&frame, false);
        accumulator.record_frame(&frame, false);
        assert_eq!(accumulator.changed_pixel_counts, vec![0, 0]);
        assert_eq!(
            accumulator.aggregate().mean_capped_changed_pixels.to_bits(),
            0.0f32.to_bits()
        );
    }

    #[test]
    fn capped_changed_pixel_mean_caps_each_transition_at_exactly_48() {
        fn one_transition(changed_pixels: usize) -> f32 {
            let mut accumulator = FrameMetricsAccumulator::default();
            let mut frame = [[0u8; BUF_SIZE]; 2];
            accumulator.record_frame(&frame, false);
            frame[0][..changed_pixels].fill(1);
            accumulator.record_frame(&frame, false);
            accumulator.aggregate().mean_capped_changed_pixels
        }

        assert_eq!(one_transition(48).to_bits(), 48.0f32.to_bits());
        assert_eq!(one_transition(49).to_bits(), 48.0f32.to_bits());
        assert_eq!(one_transition(BUF_SIZE).to_bits(), 48.0f32.to_bits());
    }

    #[test]
    fn capped_changed_pixel_mean_includes_zero_change_transitions() {
        let mut accumulator = FrameMetricsAccumulator::default();
        let mut frame = [[0u8; BUF_SIZE]; 2];
        accumulator.record_frame(&frame, false);

        frame[0][..48].fill(1);
        accumulator.record_frame(&frame, false);
        accumulator.record_frame(&frame, false);

        assert_eq!(accumulator.changed_pixel_counts, vec![48, 0]);
        assert_eq!(
            accumulator.aggregate().mean_capped_changed_pixels.to_bits(),
            24.0f32.to_bits()
        );
    }

    #[test]
    fn capped_changed_pixel_mean_preserves_steady_subcap_values() {
        let mut accumulator = FrameMetricsAccumulator::default();
        let mut frame = [[0u8; BUF_SIZE]; 2];
        accumulator.record_frame(&frame, false);

        for _ in 0..8 {
            for pixel in &mut frame[0][..17] {
                *pixel ^= 1;
            }
            accumulator.record_frame(&frame, false);
        }

        assert_eq!(accumulator.changed_pixel_counts, vec![17; 8]);
        assert_eq!(
            accumulator.aggregate().mean_capped_changed_pixels.to_bits(),
            17.0f32.to_bits()
        );
    }

    #[test]
    fn broad_transition_share_requires_both_axis_thresholds() {
        fn one_transition(rows: usize, columns: usize) -> f32 {
            let mut accumulator = FrameMetricsAccumulator::default();
            let mut frame = [[0u8; BUF_SIZE]; 2];
            accumulator.record_frame(&frame, false);
            for y in 0..rows {
                for x in 0..columns {
                    frame[0][y * HIRES_W + x] = 1;
                }
            }
            accumulator.record_frame(&frame, false);
            accumulator.aggregate().broad_transition_share
        }

        assert_eq!(one_transition(5, 12).to_bits(), 0.0f32.to_bits());
        assert_eq!(one_transition(6, 11).to_bits(), 0.0f32.to_bits());
        assert_eq!(one_transition(6, 12).to_bits(), 1.0f32.to_bits());
        assert_eq!(one_transition(HIRES_H, HIRES_W).to_bits(), 1.0f32.to_bits());
    }

    #[test]
    fn broad_transition_share_excludes_idle_transitions() {
        let mut accumulator = FrameMetricsAccumulator::default();
        let mut frame = [[0u8; BUF_SIZE]; 2];
        accumulator.record_frame(&frame, false);

        for y in 0..6 {
            for x in 0..12 {
                frame[0][y * HIRES_W + x] = 1;
            }
        }
        accumulator.record_frame(&frame, false);
        accumulator.record_frame(&frame, false);

        frame[0][0] ^= 1;
        accumulator.record_frame(&frame, false);
        accumulator.record_frame(&frame, false);

        assert_eq!(accumulator.active_transition_count, 2);
        assert_eq!(accumulator.broad_transition_count, 1);
        assert_eq!(
            accumulator.aggregate().broad_transition_share.to_bits(),
            0.5f32.to_bits()
        );
    }

    #[test]
    fn frame_accumulator_reports_pacing_variation() {
        let mut accumulator = FrameMetricsAccumulator::default();
        let mut frame = [[0u8; BUF_SIZE]; 2];
        accumulator.record_frame(&frame, false);
        frame[0][0] = 1;
        accumulator.record_frame(&frame, false);
        frame[0][0] = 0;
        accumulator.record_frame(&frame, false);
        assert_eq!(accumulator.aggregate().frame_delta_cv, 0.0);

        frame[0].fill(1);
        accumulator.record_frame(&frame, false);
        assert!(accumulator.aggregate().frame_delta_cv > 1.0);
    }

    #[test]
    fn frame_accumulator_reports_sustained_motion_spread() {
        let mut accumulator = FrameMetricsAccumulator::default();
        let mut frame = [[0u8; BUF_SIZE]; 2];
        accumulator.record_frame(&frame, false);
        for index in 0..12 {
            frame[0][index] ^= 1;
            accumulator.record_frame(&frame, false);
        }

        let spread = accumulator.aggregate().motion_spread;
        assert!((spread - 1.0).abs() < 1e-6, "spread was {spread}");
    }

    #[test]
    fn frame_accumulator_maps_changed_pixels_to_physical_regions() {
        let mut accumulator = FrameMetricsAccumulator::default();
        let mut frame = [[0u8; BUF_SIZE]; 2];
        accumulator.record_frame(&frame, false);
        assert!(accumulator.changed_region_counts.is_empty());

        for (x, y) in [
            (0, 0),
            (15, 15),
            (16, 15),
            (127, 0),
            (15, 16),
            (16, 16),
            (0, 63),
            (127, 63),
        ] {
            frame[0][y * HIRES_W + x] = 1;
        }
        accumulator.record_frame(&frame, false);

        let mut expected = [0u16; REGION_COUNT];
        expected[0] = 2;
        expected[1] = 1;
        expected[7] = 1;
        expected[8] = 1;
        expected[9] = 1;
        expected[24] = 1;
        expected[31] = 1;
        assert_eq!(accumulator.changed_region_counts, vec![expected]);
    }

    #[test]
    fn frame_accumulator_counts_full_region_traversal() {
        let mut accumulator = FrameMetricsAccumulator::default();
        let mut frame = [[0u8; BUF_SIZE]; 2];
        accumulator.record_frame(&frame, false);
        frame[0].fill(1);
        accumulator.record_frame(&frame, false);
        assert_eq!(accumulator.changed_region_counts, vec![[256; REGION_COUNT]]);

        frame[0].fill(0);
        accumulator.record_frame(&frame, false);
        assert_eq!(
            accumulator.changed_region_counts,
            vec![[256; REGION_COUNT], [256; REGION_COUNT]]
        );
        assert_eq!(
            accumulator.aggregate().motion_spatial_spread.to_bits(),
            1.0f32.to_bits()
        );
        assert_eq!(
            accumulator.aggregate().motion_axis_balance.to_bits(),
            1.0f32.to_bits()
        );
    }

    #[test]
    fn frame_loop_periodicity_rejects_static_flicker_and_noise() {
        assert_eq!(frame_loop_periodicity(&vec![7; 60], 0.1), (0, 0.0));

        let flicker: Vec<u64> = (0..60).map(|index| (index % 2) as u64).collect();
        assert_eq!(frame_loop_periodicity(&flicker, 0.1), (0, 0.0));

        let noise: Vec<u64> = (0..360).map(|index| index as u64).collect();
        assert_eq!(frame_loop_periodicity(&noise, 0.5), (0, 0.0));
    }

    #[test]
    fn frame_loop_periodicity_finds_fundamental_active_loop() {
        let hashes: Vec<u64> = (0..120).map(|index| (index % 20) as u64).collect();
        let (period, strength) = frame_loop_periodicity(&hashes, 0.01);
        assert_eq!(period, 20);
        assert!((strength - 1.0).abs() < 1e-6, "strength was {strength}");
    }

    #[test]
    fn frame_loop_periodicity_finds_long_imperfect_demo_cycle() {
        let period = 403usize;
        let hashes: Vec<u64> = (0..1_800)
            .map(|index| {
                if index % 11 == 0 {
                    10_000 + index as u64
                } else {
                    (index % period) as u64
                }
            })
            .collect();
        let (found, strength) = frame_loop_periodicity(&hashes, 0.01);
        assert_eq!(found, period as u32);
        assert!(strength > 0.80, "strength was {strength}");
    }

    #[test]
    fn event_counts_include_visual_and_timer_techniques() {
        let events = vec![
            Event::ClearScreen,
            Event::Draw {
                x: 1,
                y: 2,
                n: 3,
                collision: true,
            },
            Event::TimerSet {
                timer: Timer::Delay,
                value: 0,
            },
            Event::TimerSet {
                timer: Timer::Delay,
                value: 4,
            },
            Event::TimerSet {
                timer: Timer::Sound,
                value: 2,
            },
            Event::ScrollDown { n: 1 },
            Event::ScrollLeft,
            Event::KeyWaitEntered,
        ];
        let counts = count_events(&events);
        assert_eq!(counts.draw_count, 1);
        assert_eq!(counts.collision_count, 1);
        assert_eq!(counts.input_opcode_count, 1);
        assert_eq!(counts.clear_count, 1);
        assert_eq!(counts.delay_timer_set_count, 2);
        assert_eq!(counts.delay_timer_nonzero_count, 1);
        assert_eq!(counts.sound_timer_set_count, 1);
        assert_eq!(counts.sound_timer_nonzero_count, 1);
        assert_eq!(counts.scroll_count, 2);
    }
}
