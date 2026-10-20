//! Module identification and element layout.
//!
//! Hardware configuration bytes come from `grid_protocol.h`. Element and LED
//! counts, and the order of element kinds, come from the `grid_module_*_ui_init`
//! functions in `grid-fw/common/src/c/grid_module.c`, and the mapping from a
//! hardware configuration byte to one of those functions comes from the
//! `grid_hwcfg_module_is_*` predicates in `grid_sys.c` together with the
//! dispatch chain in `esp32s3/main/grid_esp32s3.c`.
//!
//! Three defined modules - TEK1, PB44 and SOFT - have no `*_ui_init` and no
//! dispatch branch, so the firmware itself logs "Unknown Module" for them.
//! They are identified here by name but carry no layout.

use crate::error::{Error, Result};
use crate::proto::EventType;

/// What a physical element on a module is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    /// Potentiometer or fader.
    Potmeter,
    Button,
    Encoder,
    /// Endless (continuous) potentiometer.
    Endless,
    Lcd,
    Touch,
    /// The per-module pseudo element that holds module-wide scripts.
    System,
}

impl ElementKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ElementKind::Potmeter => "potmeter",
            ElementKind::Button => "button",
            ElementKind::Encoder => "encoder",
            ElementKind::Endless => "endless",
            ElementKind::Lcd => "lcd",
            ElementKind::Touch => "touch",
            ElementKind::System => "system",
        }
    }

    /// Whether this crate can address an LED colour for this element kind.
    ///
    /// The four control kinds each own one LED whose colour lives in their
    /// event scripts. `Touch` physically drives LEDs on ZONA, but through an
    /// 81-LED matrix with no per-element value event, so it is not addressable
    /// through the colour API and is excluded here deliberately.
    pub fn has_led(self) -> bool {
        matches!(
            self,
            ElementKind::Potmeter
                | ElementKind::Button
                | ElementKind::Encoder
                | ElementKind::Endless
        )
    }

    /// The event whose script carries this element's LED and value settings.
    ///
    /// This is where the firmware's default action strings put the colour
    /// block, so it is also where a persistent colour belongs.
    pub fn value_event(self) -> Option<EventType> {
        Some(match self {
            ElementKind::Potmeter => EventType::Potmeter,
            ElementKind::Button => EventType::Button,
            ElementKind::Encoder => EventType::Encoder,
            ElementKind::Endless => EventType::Endless,
            _ => return None,
        })
    }

    /// Prefix the firmware's Lua uses for this element kind's value
    /// accessors, e.g. `"e"` gives `eva`, `emi` and `ema` on an encoder.
    ///
    /// These are the prefixes `midirx_register` and `simplecolor.lua` key
    /// their layer and value lookups on (`{ ep = 2, e = 2, p = 1, b = 1 }`).
    pub fn value_prefix(self) -> Option<&'static str> {
        Some(match self {
            ElementKind::Encoder => "e",
            ElementKind::Button => "b",
            ElementKind::Potmeter => "p",
            ElementKind::Endless => "ep",
            _ => return None,
        })
    }

    /// Events this element kind has a script slot for.
    pub fn events(self) -> &'static [EventType] {
        match self {
            ElementKind::Encoder => &[
                EventType::Init,
                EventType::Encoder,
                EventType::Button,
                EventType::Timer,
            ],
            ElementKind::Potmeter => &[EventType::Init, EventType::Potmeter, EventType::Timer],
            ElementKind::Button => &[EventType::Init, EventType::Button, EventType::Timer],
            ElementKind::Endless => &[
                EventType::Init,
                EventType::Endless,
                EventType::Button,
                EventType::Timer,
            ],
            ElementKind::System => &[EventType::Init, EventType::Mapmode, EventType::Timer],
            ElementKind::Lcd => &[EventType::Init, EventType::Draw, EventType::Timer],
            ElementKind::Touch => &[EventType::Init, EventType::Timer],
        }
    }
}

impl std::fmt::Display for ElementKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Module family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ModuleKind {
    Po16,
    Bu16,
    Pbf4,
    En16,
    Ef44,
    Octv,
    Zona,
    Tek1,
    Tek2,
    Vsn1L,
    Vsn1R,
    Vsn2,
    Pb44,
    Soft,
}

impl ModuleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ModuleKind::Po16 => "PO16",
            ModuleKind::Bu16 => "BU16",
            ModuleKind::Pbf4 => "PBF4",
            ModuleKind::En16 => "EN16",
            ModuleKind::Ef44 => "EF44",
            ModuleKind::Octv => "OCTV",
            ModuleKind::Zona => "ZONA",
            ModuleKind::Tek1 => "TEK1",
            ModuleKind::Tek2 => "TEK2",
            ModuleKind::Vsn1L => "VSN1L",
            ModuleKind::Vsn1R => "VSN1R",
            ModuleKind::Vsn2 => "VSN2",
            ModuleKind::Pb44 => "PB44",
            ModuleKind::Soft => "SOFT",
        }
    }

    /// One-line human description of the control surface.
    pub fn describe(self) -> &'static str {
        match self {
            ModuleKind::Po16 => "16 potentiometers",
            ModuleKind::Bu16 => "16 buttons",
            ModuleKind::Pbf4 => "8 potentiometers/faders, 4 buttons",
            ModuleKind::En16 => "16 rotary encoders",
            ModuleKind::Ef44 => "4 rotary encoders, 4 faders",
            ModuleKind::Octv => "8 rotary encoders, 13 buttons",
            ModuleKind::Zona => "touch surface with an 81-LED matrix",
            ModuleKind::Tek1 => "TEK1 (no layout in firmware)",
            ModuleKind::Tek2 => "8 buttons, 2 endless potentiometers",
            ModuleKind::Vsn1L => "8+4 buttons, 1 endless potentiometer, 1 LCD (left)",
            ModuleKind::Vsn1R => "8+4 buttons, 1 endless potentiometer, 1 LCD (right)",
            ModuleKind::Vsn2 => "12+4 buttons, 2 LCDs",
            ModuleKind::Pb44 => "PB44 (no layout in firmware)",
            ModuleKind::Soft => "virtual/software module (no layout in firmware)",
        }
    }
}

impl std::fmt::Display for ModuleKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a module's elements are laid out, as consecutive runs of one kind.
///
/// The runs cover every element index including the trailing system element,
/// in the exact order the firmware's `*_ui_init` assigns them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Runs(&'static [(ElementKind, u8)]),
    /// The firmware defines this module but never initialises a UI for it.
    Unknown,
}

use ElementKind::{Button, Encoder, Endless, Lcd, Potmeter, System, Touch};

/// `grid_module_po16_ui_init`: 16 potmeters, then system.
const PO16: Layout = Layout::Runs(&[(Potmeter, 16), (System, 1)]);
/// `grid_module_bu16_ui_init`: 16 buttons, then system.
const BU16: Layout = Layout::Runs(&[(Button, 16), (System, 1)]);
/// `grid_module_pbf4_ui_init`: 8 potmeters/faders, 4 buttons, then system.
const PBF4: Layout = Layout::Runs(&[(Potmeter, 8), (Button, 4), (System, 1)]);
/// `grid_module_ef44_ui_init`: 4 encoders, 4 faders, then system.
const EF44: Layout = Layout::Runs(&[(Encoder, 4), (Potmeter, 4), (System, 1)]);
/// `grid_module_en16_ui_init`: 16 encoders, then system.
const EN16: Layout = Layout::Runs(&[(Encoder, 16), (System, 1)]);
/// `grid_module_octv_ui_init`: 8 encoders, 13 buttons, then system.
const OCTV: Layout = Layout::Runs(&[(Encoder, 8), (Button, 13), (System, 1)]);
/// `grid_module_zona_ui_init`: one touch element, then system.
const ZONA: Layout = Layout::Runs(&[(Touch, 1), (System, 1)]);
/// `grid_module_vsnl_ui_init` / `_vsnr_`: 8 buttons, 1 endless, 4 buttons,
/// 1 LCD, then system. The left and right variants differ only in LED lookup.
const VSN1: Layout = Layout::Runs(&[
    (Button, 8),
    (Endless, 1),
    (Button, 4),
    (Lcd, 1),
    (System, 1),
]);
/// `grid_module_vsn2_ui_init`: 12 buttons, 1 LCD, 4 buttons, 1 LCD, system.
const VSN2: Layout = Layout::Runs(&[
    (Button, 12),
    (Lcd, 1),
    (Button, 4),
    (Lcd, 1),
    (System, 1),
]);
/// `grid_module_tek2_ui_init`: 8 buttons, 2 endless, then system.
const TEK2: Layout = Layout::Runs(&[(Button, 8), (Endless, 2), (System, 1)]);

/// A module as identified by its hardware configuration byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Module {
    pub hwcfg: u8,
    pub kind: ModuleKind,
    /// Hardware revision letter, e.g. `"D"`.
    pub revision: &'static str,
    /// Hardware sub-variant as named by the firmware (`"ND"`, `"CD"`).
    ///
    /// The firmware dispatches variants to the same `*_ui_init`, so a variant
    /// never changes the element or LED layout.
    pub variant: Option<&'static str>,
    /// Number of addressable LEDs, or 0 when the layout is unknown.
    pub led_count: u8,
    layout: Layout,
}

macro_rules! module {
    ($hwcfg:expr, $kind:expr, $rev:expr, $variant:expr, $leds:expr, $layout:expr) => {
        Module {
            hwcfg: $hwcfg,
            kind: $kind,
            revision: $rev,
            variant: $variant,
            led_count: $leds,
            layout: $layout,
        }
    };
}

/// Every hardware configuration byte `grid_protocol.h` defines.
pub const MODULES: &[Module] = &[
    // PO16 - grid_hwcfg_module_is_po16
    module!(0, ModuleKind::Po16, "B", None, 16, PO16),
    module!(1, ModuleKind::Po16, "D", None, 16, PO16),
    module!(3, ModuleKind::Po16, "H", None, 16, PO16),
    module!(6, ModuleKind::Po16, "I", None, 16, PO16),
    module!(8, ModuleKind::Po16, "C", None, 16, PO16),
    module!(11, ModuleKind::Po16, "H", Some("CD"), 16, PO16),
    module!(14, ModuleKind::Po16, "I", Some("CD"), 16, PO16),
    // BU16 - grid_hwcfg_module_is_bu16
    module!(128, ModuleKind::Bu16, "B", None, 16, BU16),
    module!(129, ModuleKind::Bu16, "D", None, 16, BU16),
    module!(131, ModuleKind::Bu16, "H", None, 16, BU16),
    module!(134, ModuleKind::Bu16, "I", None, 16, BU16),
    module!(136, ModuleKind::Bu16, "C", None, 16, BU16),
    // PBF4 - grid_hwcfg_module_is_pbf4
    module!(64, ModuleKind::Pbf4, "A", None, 12, PBF4),
    module!(65, ModuleKind::Pbf4, "D", None, 12, PBF4),
    module!(67, ModuleKind::Pbf4, "H", None, 12, PBF4),
    module!(75, ModuleKind::Pbf4, "H", Some("CD"), 12, PBF4),
    // EN16 - grid_hwcfg_module_is_en16
    module!(192, ModuleKind::En16, "A", None, 16, EN16),
    module!(193, ModuleKind::En16, "D", None, 16, EN16),
    module!(195, ModuleKind::En16, "H", None, 16, EN16),
    module!(200, ModuleKind::En16, "A", Some("ND"), 16, EN16),
    module!(201, ModuleKind::En16, "D", Some("ND"), 16, EN16),
    module!(203, ModuleKind::En16, "H", Some("ND"), 16, EN16),
    // EF44 - grid_hwcfg_module_is_ef44
    module!(32, ModuleKind::Ef44, "A", None, 8, EF44),
    module!(33, ModuleKind::Ef44, "D", None, 8, EF44),
    module!(35, ModuleKind::Ef44, "H", None, 8, EF44),
    module!(38, ModuleKind::Ef44, "I", None, 8, EF44),
    module!(41, ModuleKind::Ef44, "D", Some("ND"), 8, EF44),
    module!(43, ModuleKind::Ef44, "H", Some("ND"), 8, EF44),
    module!(46, ModuleKind::Ef44, "I", Some("ND"), 8, EF44),
    // OCTV - grid_hwcfg_module_is_octv
    module!(211, ModuleKind::Octv, "H", None, 21, OCTV),
    module!(219, ModuleKind::Octv, "H", Some("ND"), 21, OCTV),
    // ZONA - grid_hwcfg_module_is_zona
    module!(161, ModuleKind::Zona, "H", None, 81, ZONA),
    // VSN1L / VSN1R / VSN2 - grid_hwcfg_module_is_vsnl / _vsnr / _vsn2
    module!(49, ModuleKind::Vsn1L, "A", None, 18, VSN1),
    module!(57, ModuleKind::Vsn1L, "B", None, 18, VSN1),
    module!(59, ModuleKind::Vsn1L, "H", None, 18, VSN1),
    module!(81, ModuleKind::Vsn1R, "A", None, 18, VSN1),
    module!(89, ModuleKind::Vsn1R, "B", None, 18, VSN1),
    module!(91, ModuleKind::Vsn1R, "H", None, 18, VSN1),
    module!(113, ModuleKind::Vsn2, "A", None, 18, VSN2),
    module!(121, ModuleKind::Vsn2, "B", None, 18, VSN2),
    module!(123, ModuleKind::Vsn2, "H", None, 18, VSN2),
    // TEK2 - grid_hwcfg_module_is_tek2
    module!(17, ModuleKind::Tek2, "A", None, 18, TEK2),
    module!(25, ModuleKind::Tek2, "B", None, 18, TEK2),
    module!(27, ModuleKind::Tek2, "H", None, 18, TEK2),
    // Defined but never dispatched to a *_ui_init by the firmware: it logs
    // "Unknown Module" for these, so no layout can be claimed.
    module!(225, ModuleKind::Tek1, "A", None, 0, Layout::Unknown),
    module!(145, ModuleKind::Pb44, "A", None, 0, Layout::Unknown),
    module!(255, ModuleKind::Soft, "A", None, 0, Layout::Unknown),
];

impl Module {
    /// Look up a module by its heartbeat `HWCFG` byte.
    pub fn from_hwcfg(hwcfg: u8) -> Result<&'static Module> {
        MODULES
            .iter()
            .find(|m| m.hwcfg == hwcfg)
            .ok_or(Error::UnknownModule(hwcfg))
    }

    /// Display name, e.g. `"EN16 RevD"` or `"PO16 RevH CD"`.
    pub fn name(&self) -> String {
        match self.variant {
            Some(v) => format!("{} Rev{} {}", self.kind, self.revision, v),
            None => format!("{} Rev{}", self.kind, self.revision),
        }
    }

    /// Whether this module's element layout is known.
    pub fn layout_known(&self) -> bool {
        !matches!(self.layout, Layout::Unknown)
    }

    /// The element runs, in index order, or an empty slice when unknown.
    pub fn runs(&self) -> &'static [(ElementKind, u8)] {
        match self.layout {
            Layout::Runs(r) => r,
            Layout::Unknown => &[],
        }
    }

    /// Number of elements, including the trailing system element.
    pub fn element_count(&self) -> u8 {
        self.runs().iter().map(|(_, n)| *n).sum()
    }

    /// Index of the system element.
    pub fn system_element(&self) -> u8 {
        self.element_count().saturating_sub(1)
    }

    /// Kind of element `index`.
    pub fn element_kind(&self, index: u8) -> Result<ElementKind> {
        let count = self.element_count();
        if count == 0 {
            return Err(Error::Other(format!(
                "element layout for {} is not known: the firmware itself does not \
                 initialise a UI for this module",
                self.name()
            )));
        }
        if index >= count {
            return Err(Error::NoSuchElement {
                module: self.kind.as_str(),
                element: index,
                count,
                last: count - 1,
            });
        }
        let mut at = 0u8;
        for (kind, n) in self.runs() {
            if index < at + n {
                return Ok(*kind);
            }
            at += n;
        }
        unreachable!("runs cover element_count")
    }

    /// All elements whose LED colour this crate can address.
    pub fn led_elements(&self) -> impl Iterator<Item = u8> + '_ {
        (0..self.element_count())
            .filter(|&i| self.element_kind(i).map(|k| k.has_led()).unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One expected layout: hwcfg, element count, LED count, spot checks.
    type LayoutCase = (u8, u8, u8, &'static [(u8, ElementKind)]);

    /// Every layout transcribed from a `grid_module_*_ui_init`.
    #[test]
    fn every_known_layout_matches_the_firmware() {
        let cases: &[LayoutCase] = &[
            (1, 17, 16, &[(0, Potmeter), (15, Potmeter), (16, System)]),
            (129, 17, 16, &[(0, Button), (15, Button), (16, System)]),
            (
                65,
                13,
                12,
                &[(0, Potmeter), (7, Potmeter), (8, Button), (11, Button), (12, System)],
            ),
            (
                33,
                9,
                8,
                &[(0, Encoder), (3, Encoder), (4, Potmeter), (7, Potmeter), (8, System)],
            ),
            (193, 17, 16, &[(0, Encoder), (15, Encoder), (16, System)]),
            (
                211,
                22,
                21,
                &[(0, Encoder), (7, Encoder), (8, Button), (20, Button), (21, System)],
            ),
            (161, 2, 81, &[(0, Touch), (1, System)]),
            (
                49,
                15,
                18,
                &[(0, Button), (7, Button), (8, Endless), (9, Button), (12, Button), (13, Lcd), (14, System)],
            ),
            (
                81,
                15,
                18,
                &[(8, Endless), (13, Lcd), (14, System)],
            ),
            (
                113,
                19,
                18,
                &[(0, Button), (11, Button), (12, Lcd), (13, Button), (16, Button), (17, Lcd), (18, System)],
            ),
            (
                17,
                11,
                18,
                &[(0, Button), (7, Button), (8, Endless), (9, Endless), (10, System)],
            ),
        ];

        for &(hwcfg, elements, leds, spots) in cases {
            let m = Module::from_hwcfg(hwcfg).expect("hwcfg in table");
            assert_eq!(m.element_count(), elements, "{} element count", m.name());
            assert_eq!(m.led_count, leds, "{} led count", m.name());
            assert_eq!(m.system_element(), elements - 1, "{} system index", m.name());
            for &(i, kind) in spots {
                assert_eq!(
                    m.element_kind(i).unwrap(),
                    kind,
                    "{} element {i}",
                    m.name()
                );
            }
        }
    }

    #[test]
    fn every_defined_hwcfg_resolves() {
        // grid_protocol.h defines 47 GRID_MODULE_* constants.
        assert_eq!(MODULES.len(), 47, "one entry per GRID_MODULE_* define");
        for m in MODULES {
            assert!(Module::from_hwcfg(m.hwcfg).is_ok());
            assert!(!m.name().is_empty());
            assert!(!m.kind.describe().is_empty());
        }
    }

    #[test]
    fn only_undispatched_modules_lack_a_layout() {
        // TEK1, PB44 and SOFT have no *_ui_init and no dispatch branch, so the
        // firmware logs "Unknown Module" for them too.
        let unknown: Vec<ModuleKind> = MODULES
            .iter()
            .filter(|m| !m.layout_known())
            .map(|m| m.kind)
            .collect();
        assert_eq!(
            unknown,
            vec![ModuleKind::Tek1, ModuleKind::Pb44, ModuleKind::Soft]
        );
    }

    #[test]
    fn runs_cover_exactly_the_element_range() {
        for m in MODULES.iter().filter(|m| m.layout_known()) {
            let total: u8 = m.runs().iter().map(|(_, n)| *n).sum();
            assert_eq!(total, m.element_count(), "{}", m.name());
            // Every index resolves, one past the end does not.
            for i in 0..m.element_count() {
                assert!(m.element_kind(i).is_ok(), "{} element {i}", m.name());
            }
            assert!(m.element_kind(m.element_count()).is_err(), "{}", m.name());
            // The last element is always the system element.
            assert_eq!(
                m.element_kind(m.system_element()).unwrap(),
                ElementKind::System,
                "{}", m.name()
            );
        }
    }

    #[test]
    fn identifies_the_connected_en16() {
        // HWCFG 0xc1 is what the 16-encoder unit on this bench reports.
        let m = Module::from_hwcfg(0xc1).unwrap();
        assert_eq!(m.kind, ModuleKind::En16);
        assert_eq!(m.name(), "EN16 RevD");
        assert_eq!(m.led_elements().count(), 16);
    }

    #[test]
    fn identifies_the_connected_ef44() {
        // HWCFG 0x21 is what the 4-fader unit on this bench reports.
        let m = Module::from_hwcfg(0x21).unwrap();
        assert_eq!(m.kind, ModuleKind::Ef44);
        assert_eq!(m.name(), "EF44 RevD");
        assert_eq!(m.led_elements().count(), 8);
    }

    #[test]
    fn lcd_and_touch_are_not_colour_addressable() {
        // ZONA drives 81 LEDs from one touch element with no value event, and
        // VSN LCDs have no LED of their own.
        assert!(!ElementKind::Touch.has_led());
        assert!(!ElementKind::Lcd.has_led());
        let zona = Module::from_hwcfg(161).unwrap();
        assert_eq!(zona.led_elements().count(), 0);
        let vsn = Module::from_hwcfg(49).unwrap();
        assert_eq!(vsn.led_elements().count(), 13, "8+1+4, excluding LCD and system");
    }

    #[test]
    fn unknown_layout_errors_explain_why() {
        let tek1 = Module::from_hwcfg(225).unwrap();
        assert!(!tek1.layout_known());
        assert_eq!(tek1.element_count(), 0);
        let err = tek1.element_kind(0).unwrap_err().to_string();
        assert!(err.contains("does not initialise"), "{err}");
    }

    #[test]
    fn rejects_out_of_range_elements() {
        let m = Module::from_hwcfg(0xc1).unwrap();
        assert!(matches!(
            m.element_kind(17),
            Err(Error::NoSuchElement { element: 17, count: 17, .. })
        ));
    }

    #[test]
    fn rejects_unknown_hwcfg() {
        assert!(matches!(Module::from_hwcfg(0x7e), Err(Error::UnknownModule(0x7e))));
    }

    #[test]
    fn hwcfg_bytes_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for m in MODULES {
            assert!(seen.insert(m.hwcfg), "duplicate hwcfg {}", m.hwcfg);
        }
    }

    #[test]
    fn element_value_events_match_firmware_defaults() {
        assert_eq!(ElementKind::Encoder.value_event(), Some(EventType::Encoder));
        assert_eq!(ElementKind::Potmeter.value_event(), Some(EventType::Potmeter));
        assert_eq!(ElementKind::Button.value_event(), Some(EventType::Button));
        assert_eq!(ElementKind::Endless.value_event(), Some(EventType::Endless));
        assert_eq!(ElementKind::System.value_event(), None);
    }

    #[test]
    fn value_prefixes_match_the_firmware_lua() {
        assert_eq!(ElementKind::Encoder.value_prefix(), Some("e"));
        assert_eq!(ElementKind::Button.value_prefix(), Some("b"));
        assert_eq!(ElementKind::Potmeter.value_prefix(), Some("p"));
        assert_eq!(ElementKind::Endless.value_prefix(), Some("ep"));
        assert_eq!(ElementKind::System.value_prefix(), None);
    }
}
