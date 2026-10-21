//! Unofficial Rust library for Intech Studio Grid controllers.
//!
//! Not affiliated with, authorised by, or endorsed by Intech Studio. The
//! protocol handling here is derived from the vendor's published firmware
//! and protocol sources, which are vendored under `vendor/` for reference.
//!
//! Static configuration, live runtime control and firmware updates over the
//! module's USB serial port. Every hardware id the firmware defines is
//! recognised (47), and 44 have a verified element layout.
//!
//! The protocol, module table, LED layers and action-string format in this
//! crate were transcribed from the open-source firmware
//! ([grid-fw](https://github.com/intechstudio/grid-fw)) and verified against
//! real hardware.
//!
//! # Example
//!
//! ```no_run
//! use intech_grid::{Device, Rgb};
//!
//! let mut dev = Device::open_single()?;
//! println!("{} on firmware {}", dev.info().module, dev.info().firmware);
//!
//! // Light encoder 3 red right now, without touching stored config.
//! dev.flash_led(3, Rgb::new(255, 0, 0), None)?;
//!
//! // Store the colour so it survives a power cycle.
//! dev.set_led_color(0, 3, "#ff0000".parse()?, 1.0)?;
//! dev.store_page()?;
//! # Ok::<(), intech_grid::Error>(())
//! ```

pub mod action;
pub mod color;
pub mod device;
pub mod error;
pub mod firmware;
pub mod frame;
pub mod live;
pub mod midi;
pub mod module;
pub mod proto;
pub mod snapshot;

pub use action::ActionString;
pub use color::Rgb;
pub use device::{discover, Device, DeviceInfo, Feature, MidiRxBinding, PortInfo};
pub use error::{Error, Result};
pub use firmware::{find_bootloader_volumes, BootloaderVolume, Uf2Image};
pub use live::{ElementSettings, KnobMode, LedAnimation};
pub use midi::{discover_midi, MidiOut};
pub use module::{ElementKind, Module, ModuleKind};
pub use proto::{layer, EventType};
pub use snapshot::{apply, dump, Snapshot};
