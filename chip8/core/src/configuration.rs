//! Native CHIP-8 identity and runtime quirk configuration.

pub const MACHINE_ID: &str = "chip8";
pub const SEMANTICS: &str = "chip8-family-semantics.v1";

/// Runtime quirks configuration.
///
/// Every flag is independent and defaults to `false` (modern/Octo-compatible
/// behaviour).  Callers may apply named presets via `QuirksConfig::preset()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QuirksConfig {
    /// `8XY6` / `8XYE`: use VX as the source operand instead of VY.
    /// False = original VIP (VY → VX).  True = SUPER-CHIP (VX in-place).
    pub shift_vx_only: bool,

    /// `FX55` / `FX65`: leave I unchanged after store/load.
    /// False = VIP (I increments).  True = SUPER-CHIP.
    pub load_store_no_inc_i: bool,

    /// `DXYN`: wait for vertical blank before updating the display.
    /// True = VIP original; causes some games to run slower but avoids flicker.
    pub display_wait: bool,

    /// Sprite drawing clips at display edges instead of wrapping.
    /// True = clip (most ROMs expect this).
    pub clipping: bool,

    /// `8XY1` / `8XY2` / `8XY3`: reset VF to 0 after the operation.
    /// True = VIP original.  False = SUPER-CHIP / modern.
    pub vf_reset_on_logic: bool,

    /// `BNNN` (jump with offset): use `VX + XNN` instead of `V0 + NNN`.
    /// False = VIP.  True = SUPER-CHIP.
    pub jump_offset_vx: bool,
}

impl Default for QuirksConfig {
    /// Modern / Octo-compatible defaults.
    fn default() -> Self {
        Self {
            shift_vx_only: false,
            load_store_no_inc_i: false,
            display_wait: false,
            clipping: true,
            vf_reset_on_logic: false,
            jump_offset_vx: false,
        }
    }
}

impl QuirksConfig {
    /// Original COSMAC VIP behaviour.
    pub fn vip() -> Self {
        Self {
            shift_vx_only: false,
            load_store_no_inc_i: false,
            display_wait: true,
            clipping: true,
            vf_reset_on_logic: true,
            jump_offset_vx: false,
        }
    }

    /// SUPER-CHIP (HP-48) behaviour.
    pub fn schip() -> Self {
        Self {
            shift_vx_only: true,
            load_store_no_inc_i: true,
            display_wait: false,
            clipping: true,
            vf_reset_on_logic: false,
            jump_offset_vx: true,
        }
    }

    /// XO-CHIP behaviour (closest to modern / Octo defaults).
    pub fn xochip() -> Self {
        Self {
            shift_vx_only: false,
            load_store_no_inc_i: false,
            display_wait: false,
            clipping: true,
            vf_reset_on_logic: false,
            jump_offset_vx: false,
        }
    }

    /// Parse a preset name string.  Returns `None` for unknown names.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "chip8" | "chip48" => Some(Self::default()),
            "vip" => Some(Self::vip()),
            "schip" => Some(Self::schip()),
            "xochip" => Some(Self::xochip()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::QuirksConfig;

    #[test]
    fn named_quirk_presets_resolve() {
        assert_eq!(
            QuirksConfig::from_name("chip8"),
            Some(QuirksConfig::default())
        );
        assert_eq!(
            QuirksConfig::from_name("chip48"),
            Some(QuirksConfig::default())
        );
        assert_eq!(QuirksConfig::from_name("vip"), Some(QuirksConfig::vip()));
        assert_eq!(
            QuirksConfig::from_name("schip"),
            Some(QuirksConfig::schip())
        );
        assert_eq!(
            QuirksConfig::from_name("xochip"),
            Some(QuirksConfig::xochip())
        );
        assert!(QuirksConfig::from_name("unknown").is_none());
    }
}
