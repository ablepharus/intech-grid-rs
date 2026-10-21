//! Protocol constants, transcribed from `grid-fw/common/src/c/grid_protocol.h`
//! (protocol version 1.5.6).

/// USB identifiers of Intech Studio Grid modules (ESP32-S3 based).
pub const USB_VID: u16 = 0x303a;
pub const USB_PID: u16 = 0x8123;

/// Framing control characters.
pub mod ctrl {
    pub const NUL: u8 = 0x00;
    pub const SOH: u8 = 0x01;
    pub const STX: u8 = 0x02;
    pub const ETX: u8 = 0x03;
    pub const EOT: u8 = 0x04;
    pub const ACK: u8 = 0x06;
    pub const BELL: u8 = 0x07;
    pub const LF: u8 = 0x0a;
    pub const DCT: u8 = 0x0e;
    pub const BRC: u8 = 0x0f;
    pub const NAK: u8 = 0x15;
    pub const EOB: u8 = 0x17;
    pub const CAN: u8 = 0x18;
}

/// Instruction nibble, at offset 4 of every body.
pub mod instr {
    pub const ACKNOWLEDGE: u8 = 0xA;
    pub const NACKNOWLEDGE: u8 = 0xB;
    pub const CHECK: u8 = 0xC;
    pub const REPORT: u8 = 0xD;
    pub const EXECUTE: u8 = 0xE;
    pub const FETCH: u8 = 0xF;
}

/// Class codes, the 3 hex digits following STX.
pub mod class {
    pub const MIDI: u16 = 0x000;
    pub const MIDISYSEX: u16 = 0x001;
    pub const MIDIRTM: u16 = 0x002;
    pub const HEARTBEAT: u16 = 0x010;
    pub const SERIALNUMBER: u16 = 0x011;
    pub const RESETCAUSE: u16 = 0x012;
    pub const RESET: u16 = 0x013;
    pub const UPTIME: u16 = 0x014;
    pub const DEBUGTEXT: u16 = 0x020;
    pub const DEBUGTASK: u16 = 0x021;
    pub const WEBSOCKET: u16 = 0x022;
    pub const PACKAGE: u16 = 0x023;
    pub const PAGEACTIVE: u16 = 0x030;
    pub const PAGECOUNT: u16 = 0x031;
    pub const LEDPREVIEW: u16 = 0x042;
    pub const EVENT: u16 = 0x050;
    pub const EVENTPREVIEW: u16 = 0x051;
    pub const ELEMENTNAME: u16 = 0x052;
    pub const EVENTVIEW: u16 = 0x053;
    pub const NAMEPREVIEW: u16 = 0x054;
    pub const CONFIG: u16 = 0x060;
    pub const PAGESTORE: u16 = 0x061;
    pub const NVMERASE: u16 = 0x062;
    pub const PAGEDISCARD: u16 = 0x063;
    pub const PAGECLEAR: u16 = 0x064;
    pub const NVMDEFRAG: u16 = 0x065;
    pub const IMMEDIATE: u16 = 0x085;
    pub const EVALUATE: u16 = 0x086;

    /// Human-readable name for a class code, for diagnostics.
    pub fn name(code: u16) -> &'static str {
        match code {
            MIDI => "MIDI",
            MIDISYSEX => "MIDISYSEX",
            MIDIRTM => "MIDIRTM",
            HEARTBEAT => "HEARTBEAT",
            SERIALNUMBER => "SERIALNUMBER",
            RESETCAUSE => "RESETCAUSE",
            RESET => "RESET",
            UPTIME => "UPTIME",
            DEBUGTEXT => "DEBUGTEXT",
            DEBUGTASK => "DEBUGTASK",
            WEBSOCKET => "WEBSOCKET",
            PACKAGE => "PACKAGE",
            PAGEACTIVE => "PAGEACTIVE",
            PAGECOUNT => "PAGECOUNT",
            LEDPREVIEW => "LEDPREVIEW",
            EVENT => "EVENT",
            EVENTPREVIEW => "EVENTPREVIEW",
            ELEMENTNAME => "ELEMENTNAME",
            EVENTVIEW => "EVENTVIEW",
            NAMEPREVIEW => "NAMEPREVIEW",
            CONFIG => "CONFIG",
            PAGESTORE => "PAGESTORE",
            NVMERASE => "NVMERASE",
            PAGEDISCARD => "PAGEDISCARD",
            PAGECLEAR => "PAGECLEAR",
            NVMDEFRAG => "NVMDEFRAG",
            IMMEDIATE => "IMMEDIATE",
            EVALUATE => "EVALUATE",
            _ => "UNKNOWN",
        }
    }
}

/// Field offsets inside the broadcast (BRC) header.
pub mod brc {
    pub const LEN: (usize, usize) = (2, 4);
    pub const ID: (usize, usize) = (6, 2);
    pub const SESSION: (usize, usize) = (8, 2);
    pub const SX: (usize, usize) = (10, 2);
    pub const SY: (usize, usize) = (12, 2);
    pub const DX: (usize, usize) = (14, 2);
    pub const DY: (usize, usize) = (16, 2);
    pub const ROT: (usize, usize) = (18, 1);
    pub const PORTROT: (usize, usize) = (19, 1);
    pub const MSGAGE: (usize, usize) = (20, 2);
    /// Total header length, including SOH, BRC and the trailing EOB.
    pub const LEN_TOTAL: usize = 23;
}

/// Field offsets inside a CONFIG (0x060) body, relative to STX.
pub mod config_fields {
    /// Id of the request being acknowledged. Shares its offset with
    /// `VERSION_MAJOR` because acknowledgements use the shorter check frame,
    /// and is also used by PAGESTORE, PAGEDISCARD and PAGECLEAR.
    pub const LASTHEADER: (usize, usize) = (5, 2);
    pub const VERSION_MAJOR: (usize, usize) = (5, 2);
    pub const VERSION_MINOR: (usize, usize) = (7, 2);
    pub const VERSION_PATCH: (usize, usize) = (9, 2);
    pub const PAGE: (usize, usize) = (11, 2);
    pub const ELEMENT: (usize, usize) = (13, 2);
    pub const EVENT: (usize, usize) = (15, 2);
    pub const ACTION_LENGTH: (usize, usize) = (17, 4);
    pub const ACTION_STRING: usize = 21;
}

/// Field offsets inside a HEARTBEAT (0x010) body, relative to STX.
pub mod heartbeat_fields {
    pub const TYPE: (usize, usize) = (5, 2);
    pub const HWCFG: (usize, usize) = (7, 2);
    pub const VERSION_MAJOR: (usize, usize) = (9, 2);
    pub const VERSION_MINOR: (usize, usize) = (11, 2);
    pub const VERSION_PATCH: (usize, usize) = (13, 2);
    pub const PORTSTATE: (usize, usize) = (15, 2);
    pub const GCCOUNT: (usize, usize) = (17, 2);
}

/// Heartbeat `TYPE` values.
pub mod heartbeat_type {
    pub const GRID: u8 = 0;
    pub const GRIDMASTER: u8 = 1;
    pub const EDITOR: u8 = 255;
}

/// Maximum length of a single action (Lua) string, per firmware.
pub const ACTIONSTRING_MAXLENGTH: usize = 909;

/// Default module position reported by a standalone module.
pub const DEFAULT_POSITION: u8 = 127;

/// LED layers. Higher layers render on top of lower ones.
pub mod layer {
    pub const ALERT: u8 = 0;
    pub const UI_A: u8 = 1;
    pub const UI_B: u8 = 2;
    pub const COUNT: u8 = 3;
}

/// Event types an element can have a script for.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Init,
    Potmeter,
    Encoder,
    Button,
    Mapmode,
    Timer,
    Endless,
    Draw,
}

impl EventType {
    /// The wire value used in the CONFIG EVENTTYPE field.
    pub fn code(self) -> u8 {
        match self {
            EventType::Init => 0,
            EventType::Potmeter => 1,
            EventType::Encoder => 2,
            EventType::Button => 3,
            EventType::Mapmode => 4,
            EventType::Timer => 6,
            EventType::Endless => 7,
            EventType::Draw => 8,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => EventType::Init,
            1 => EventType::Potmeter,
            2 => EventType::Encoder,
            3 => EventType::Button,
            4 => EventType::Mapmode,
            6 => EventType::Timer,
            7 => EventType::Endless,
            8 => EventType::Draw,
            _ => return None,
        })
    }

    /// Short Lua handler name the firmware uses for this event.
    pub fn handler(self) -> &'static str {
        match self {
            EventType::Init => "ini",
            EventType::Potmeter => "pc",
            EventType::Encoder => "ec",
            EventType::Button => "bc",
            EventType::Mapmode => "map",
            EventType::Timer => "tim",
            EventType::Endless => "epc",
            EventType::Draw => "ld",
        }
    }

    /// LED layer that `glc(-1, ..)` resolves to inside this event handler.
    ///
    /// Mirrors `event_handler_to_layer` in `grid-fw/common/src/lua/simplecolor.lua`.
    pub fn auto_layer(self) -> Option<u8> {
        match self {
            EventType::Encoder | EventType::Endless => Some(layer::UI_B),
            EventType::Button | EventType::Potmeter => Some(layer::UI_A),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EventType::Init => "init",
            EventType::Potmeter => "potmeter",
            EventType::Encoder => "encoder",
            EventType::Button => "button",
            EventType::Mapmode => "mapmode",
            EventType::Timer => "timer",
            EventType::Endless => "endless",
            EventType::Draw => "draw",
        }
    }
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for EventType {
    type Err = crate::Error;

    fn from_str(s: &str) -> crate::Result<Self> {
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "init" => EventType::Init,
            "potmeter" | "fader" | "pot" => EventType::Potmeter,
            "encoder" => EventType::Encoder,
            "button" => EventType::Button,
            "mapmode" => EventType::Mapmode,
            "timer" => EventType::Timer,
            "endless" => EventType::Endless,
            "draw" => EventType::Draw,
            other => {
                return Err(crate::Error::Other(format!(
                    "unknown event type {other:?} (expected init, potmeter, encoder, button, mapmode, timer, endless or draw)"
                )))
            }
        })
    }
}
