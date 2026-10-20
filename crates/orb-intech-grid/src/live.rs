//! Live and stored control of how elements behave.
//!
//! This module carries the *semantics* of the element settings - what the
//! firmware's numeric modes actually mean - so callers do not have to pass
//! raw integers. The transport lives on [`crate::Device`].

use crate::error::Error;

/// How an encoder or endless potentiometer turns movement into a value.
///
/// The numbering is the firmware's, from the `mode` branches in
/// `grid-fw/common/src/c/grid_ui_encoder.c`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnobMode {
    /// `0` - absolute. The value accumulates and is clamped to `min..=max`,
    /// and nothing resets it, so it continues from wherever it was. This is
    /// the firmware default.
    Absolute,
    /// `1` - relative. Movement accumulates exactly as in absolute mode, but
    /// the value is re-centred to `((max + 1) + min) / 2` whenever the
    /// element's event is initialised, giving a knob that springs back to the
    /// middle of its range.
    Relative,
    /// `2` - signed relative (two's complement). The stored value encodes a
    /// *signed delta* over the range rather than a position, clamped to
    /// `-range/2 ..= range/2 - 1`, and initialises to `min`. This is the mode
    /// DAWs expect for endless-encoder style "turn delta" control.
    RelativeSigned,
}

impl KnobMode {
    /// The firmware's numeric mode value.
    pub fn code(self) -> i64 {
        match self {
            KnobMode::Absolute => 0,
            KnobMode::Relative => 1,
            KnobMode::RelativeSigned => 2,
        }
    }

    /// Interpret a firmware mode value.
    pub fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            0 => KnobMode::Absolute,
            1 => KnobMode::Relative,
            2 => KnobMode::RelativeSigned,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            KnobMode::Absolute => "absolute",
            KnobMode::Relative => "relative",
            KnobMode::RelativeSigned => "relative-signed",
        }
    }
}

impl std::fmt::Display for KnobMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for KnobMode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "absolute" | "abs" | "0" => KnobMode::Absolute,
            "relative" | "rel" | "1" => KnobMode::Relative,
            "relative-signed" | "signed" | "twos" | "2" => KnobMode::RelativeSigned,
            other => {
                return Err(Error::Other(format!(
                    "unknown knob mode {other:?} (expected absolute, relative or relative-signed)"
                )))
            }
        })
    }
}

/// An element's stored behaviour settings, as read back from the device.
///
/// Every field is `None` when the element's script does not set it, which is
/// how the firmware expresses "use the element default".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct ElementSettings {
    /// Movement mode, when the element has one.
    pub mode: Option<KnobMode>,
    /// Raw mode value, kept even when it is not a [`KnobMode`] this crate
    /// knows, so nothing is silently lost.
    pub mode_raw: Option<i64>,
    pub min: Option<i64>,
    pub max: Option<i64>,
    /// Encoder/endless sensitivity, in percent of the default.
    pub sensitivity: Option<i64>,
    /// Encoder/endless velocity scaling.
    pub velocity: Option<i64>,
}

/// LED animation parameters, applied per LED layer.
///
/// These map to the firmware's global `glf` (`led_animation_rate`) and `gls`
/// (`led_animation_type`) functions, which take `(led_index, layer, value)`.
/// The firmware clears both to `0` when it initialises an LED, so `0` is the
/// steady, non-animating setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedAnimation {
    /// Animation rate, `glf`. `0` is steady.
    pub rate: u8,
    /// Animation type/shape, `gls`. `0` is steady.
    pub shape: u8,
}

impl LedAnimation {
    /// No animation: both parameters at the firmware's cleared value.
    pub const STEADY: LedAnimation = LedAnimation { rate: 0, shape: 0 };

    pub fn new(rate: u8, shape: u8) -> Self {
        LedAnimation { rate, shape }
    }

    /// Whether this is the steady (non-animating) setting.
    pub fn is_steady(self) -> bool {
        self == LedAnimation::STEADY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_codes_match_the_firmware_branches() {
        // grid_ui_encoder.c: default 0; `== 1` is commented "relative";
        // `== 2` stores a two's-complement signed delta.
        assert_eq!(KnobMode::Absolute.code(), 0);
        assert_eq!(KnobMode::Relative.code(), 1);
        assert_eq!(KnobMode::RelativeSigned.code(), 2);
        for m in [KnobMode::Absolute, KnobMode::Relative, KnobMode::RelativeSigned] {
            assert_eq!(KnobMode::from_code(m.code()), Some(m));
        }
        assert_eq!(KnobMode::from_code(7), None);
    }

    #[test]
    fn mode_parses_every_accepted_spelling() {
        for s in ["absolute", "ABS", "0"] {
            assert_eq!(s.parse::<KnobMode>().unwrap(), KnobMode::Absolute);
        }
        for s in ["relative", "rel", "1"] {
            assert_eq!(s.parse::<KnobMode>().unwrap(), KnobMode::Relative);
        }
        for s in ["relative-signed", "signed", "twos", "2"] {
            assert_eq!(s.parse::<KnobMode>().unwrap(), KnobMode::RelativeSigned);
        }
        assert!("sideways".parse::<KnobMode>().is_err());
    }

    #[test]
    fn steady_animation_is_the_firmware_cleared_state() {
        assert!(LedAnimation::STEADY.is_steady());
        assert_eq!(LedAnimation::STEADY.rate, 0);
        assert_eq!(LedAnimation::STEADY.shape, 0);
        assert!(!LedAnimation::new(3, 1).is_steady());
    }
}
