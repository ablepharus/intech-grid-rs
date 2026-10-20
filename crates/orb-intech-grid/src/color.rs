//! RGB colours and their textual forms.

use crate::error::{Error, Result};

/// An 8-bit-per-channel colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Colour names accepted on the command line. The first seven match the
/// `GRID_LED_COLOR_*` macros in the firmware.
pub const NAMED: &[(&str, Rgb)] = &[
    ("off", Rgb::new(0, 0, 0)),
    ("black", Rgb::new(0, 0, 0)),
    ("red", Rgb::new(255, 0, 0)),
    ("green", Rgb::new(0, 255, 0)),
    ("blue", Rgb::new(0, 0, 255)),
    ("yellow", Rgb::new(255, 255, 0)),
    ("yellow-dim", Rgb::new(64, 64, 0)),
    ("purple", Rgb::new(255, 0, 255)),
    ("magenta", Rgb::new(255, 0, 255)),
    ("white", Rgb::new(255, 255, 255)),
    ("white-dim", Rgb::new(64, 64, 64)),
    ("cyan", Rgb::new(0, 255, 255)),
    ("orange", Rgb::new(255, 96, 0)),
    ("pink", Rgb::new(255, 64, 128)),
    ("lila", Rgb::new(160, 64, 255)),
    ("violet", Rgb::new(160, 64, 255)),
    ("teal", Rgb::new(0, 160, 160)),
    ("amber", Rgb::new(255, 160, 0)),
];

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }

    pub const BLACK: Rgb = Rgb::new(0, 0, 0);

    /// `#rrggbb`.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Scale all channels by `factor`, clamped to `0.0..=1.0`.
    pub fn scaled(self, factor: f32) -> Self {
        let f = factor.clamp(0.0, 1.0);
        Rgb {
            r: (self.r as f32 * f).round() as u8,
            g: (self.g as f32 * f).round() as u8,
            b: (self.b as f32 * f).round() as u8,
        }
    }

    /// List of accepted colour names, for help text.
    pub fn names() -> Vec<&'static str> {
        NAMED.iter().map(|(n, _)| *n).collect()
    }
}

impl std::fmt::Display for Rgb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl std::str::FromStr for Rgb {
    type Err = Error;

    /// Accepts `red`, `#ff0000`, `ff0000`, `f00`, `255,0,0` and `255 0 0`.
    fn from_str(s: &str) -> Result<Self> {
        let t = s.trim();
        if t.is_empty() {
            return Err(Error::Color(s.to_string()));
        }

        let lower = t.to_ascii_lowercase();
        if let Some((_, rgb)) = NAMED.iter().find(|(n, _)| *n == lower) {
            return Ok(*rgb);
        }

        if t.contains(',') || t.contains(char::is_whitespace) {
            let parts: Vec<&str> = t
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|p| !p.is_empty())
                .collect();
            if parts.len() != 3 {
                return Err(Error::Color(s.to_string()));
            }
            let mut ch = [0u8; 3];
            for (i, p) in parts.iter().enumerate() {
                ch[i] = p.parse::<u8>().map_err(|_| Error::Color(s.to_string()))?;
            }
            return Ok(Rgb::new(ch[0], ch[1], ch[2]));
        }

        let hex = lower.strip_prefix('#').unwrap_or(&lower);
        let bytes = match hex.len() {
            6 => hex.to_string(),
            // #abc -> #aabbcc
            3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
            _ => return Err(Error::Color(s.to_string())),
        };
        if !bytes.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::Color(s.to_string()));
        }
        let v = u32::from_str_radix(&bytes, 16).map_err(|_| Error::Color(s.to_string()))?;
        Ok(Rgb::new((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_accepted_form() {
        let red = Rgb::new(255, 0, 0);
        for form in ["red", "RED", "#ff0000", "ff0000", "#f00", "f00", "255,0,0", "255, 0, 0", "255 0 0"] {
            assert_eq!(form.parse::<Rgb>().unwrap(), red, "parsing {form:?}");
        }
    }

    #[test]
    fn named_colours_match_firmware_macros() {
        // GRID_LED_COLOR_* in grid_led.h
        assert_eq!("purple".parse::<Rgb>().unwrap(), Rgb::new(255, 0, 255));
        assert_eq!("yellow".parse::<Rgb>().unwrap(), Rgb::new(255, 255, 0));
        assert_eq!("yellow-dim".parse::<Rgb>().unwrap(), Rgb::new(64, 64, 0));
        assert_eq!("white-dim".parse::<Rgb>().unwrap(), Rgb::new(64, 64, 64));
    }

    #[test]
    fn rejects_nonsense() {
        for bad in ["", "nope", "#12345", "300,0,0", "1,2", "1,2,3,4", "#gggggg"] {
            assert!(bad.parse::<Rgb>().is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn hex_round_trips() {
        let c = Rgb::new(0x12, 0xab, 0xef);
        assert_eq!(c.to_hex(), "#12abef");
        assert_eq!(c.to_hex().parse::<Rgb>().unwrap(), c);
    }

    #[test]
    fn scaling_clamps() {
        assert_eq!(Rgb::new(200, 100, 50).scaled(0.5), Rgb::new(100, 50, 25));
        assert_eq!(Rgb::new(200, 100, 50).scaled(2.0), Rgb::new(200, 100, 50));
        assert_eq!(Rgb::new(200, 100, 50).scaled(-1.0), Rgb::BLACK);
    }
}
