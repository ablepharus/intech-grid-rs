use std::fmt;

/// Errors produced by this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("serial port error: {0}")]
    Serial(#[from] serialport::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("no Intech Grid device found (looked for USB {vid:04x}:{pid:04x} serial ports)")]
    NoDevice { vid: u16, pid: u16 },

    #[error("malformed packet: {0}")]
    Malformed(String),

    #[error("checksum mismatch: computed {computed:02x}, frame says {stated:02x}")]
    Checksum { computed: u8, stated: u8 },

    #[error("action string is {len} bytes, firmware limit is {max}")]
    ActionTooLong { len: usize, max: usize },

    #[error("timed out after {ms} ms waiting for {what}")]
    Timeout { what: &'static str, ms: u64 },

    #[error("unknown hardware configuration byte 0x{0:02x}")]
    UnknownModule(u8),

    #[error("element {element} does not exist on {module} (has {count} elements, 0..{last})")]
    NoSuchElement {
        module: &'static str,
        element: u8,
        count: u8,
        last: u8,
    },

    #[error("element {element} on {module} is a {kind}, which has no {what}")]
    Unsupported {
        module: &'static str,
        element: u8,
        kind: &'static str,
        what: &'static str,
    },

    #[error("invalid color {0:?}: expected a name like 'red', '#rrggbb', 'rrggbb' or 'r,g,b'")]
    Color(String),

    #[error("device rejected the {0} request (NAK)")]
    Nak(&'static str),

    #[error(
        "this module's firmware {found} does not provide {feature}; it needs {needs} or newer"
    )]
    UnsupportedFirmware {
        feature: &'static str,
        found: String,
        needs: &'static str,
    },

    #[error("{0}")]
    Other(String),
}

/// Convenient result alias.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn malformed(msg: impl fmt::Display) -> Self {
        Error::Malformed(msg.to_string())
    }
}
