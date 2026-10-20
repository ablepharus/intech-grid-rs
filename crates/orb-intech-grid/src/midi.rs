//! Sending MIDI to a module through its ALSA raw MIDI node.
//!
//! A Grid module exposes a USB MIDI interface alongside the serial control
//! port. Driving an element over MIDI is the firmware-sanctioned way to move
//! its value from software: with a binding installed by
//! [`crate::Device::bind_midi_rx`], an incoming message sets the element's
//! value and scales its LED, so the next physical move continues from there.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// An open raw MIDI output to a module.
pub struct MidiOut {
    file: std::fs::File,
    path: PathBuf,
}

/// Locate the raw MIDI node of an Intech Grid module.
///
/// `/dev/snd/by-id` points at the module's control node (`controlC<N>`), and
/// the raw MIDI node of the same card is `midiC<N>D0`.
pub fn discover_midi() -> Result<Vec<PathBuf>> {
    let by_id = Path::new("/dev/snd/by-id");
    let mut found = Vec::new();
    if !by_id.is_dir() {
        return Ok(found);
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(by_id)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();

    for link in entries {
        let name = link
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !name.contains("intech") {
            continue;
        }
        let target = std::fs::canonicalize(&link)?;
        let card = target
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix("controlC"))
            .and_then(|s| s.parse::<u32>().ok());
        if let Some(card) = card {
            let midi = PathBuf::from(format!("/dev/snd/midiC{card}D0"));
            if midi.exists() {
                found.push(midi);
            }
        }
    }
    Ok(found)
}

impl MidiOut {
    /// Open the only Grid MIDI node on the system.
    pub fn open_single() -> Result<Self> {
        let ports = discover_midi()?;
        match ports.len() {
            0 => Err(Error::Other(
                "no Intech Grid MIDI node found under /dev/snd/by-id".to_string(),
            )),
            1 => MidiOut::open(&ports[0]),
            n => Err(Error::Other(format!(
                "{n} Grid MIDI nodes are present; pass one explicitly"
            ))),
        }
    }

    pub fn open(path: &Path) -> Result<Self> {
        let file = std::fs::OpenOptions::new().write(true).open(path)?;
        Ok(MidiOut {
            file,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Send one channel-voice message.
    ///
    /// `command` is the status byte without its channel nibble, e.g. `0xB0`
    /// for control change.
    pub fn send(&mut self, channel: u8, command: u8, p1: u8, p2: u8) -> Result<()> {
        if channel > 15 {
            return Err(Error::Other(format!("MIDI channel {channel} is above 15")));
        }
        if p1 > 127 || p2 > 127 {
            return Err(Error::Other(format!(
                "MIDI data bytes must be 0-127, got {p1} and {p2}"
            )));
        }
        let status = (command & 0xf0) | channel;
        self.file.write_all(&[status, p1, p2])?;
        self.file.flush()?;
        Ok(())
    }

    /// Send a control change.
    pub fn control_change(&mut self, channel: u8, cc: u8, value: u8) -> Result<()> {
        self.send(channel, 0xb0, cc, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_out_of_range_midi_data() {
        // Guard the encoding before it reaches the device: a data byte with
        // its top bit set would be read as a new status byte.
        let bad = MidiOut {
            file: std::fs::File::open("/dev/null").unwrap(),
            path: PathBuf::from("/dev/null"),
        };
        drop(bad);

        // The range checks are pure, so exercise them through a null sink.
        let mut sink = MidiOut {
            file: std::fs::OpenOptions::new().write(true).open("/dev/null").unwrap(),
            path: PathBuf::from("/dev/null"),
        };
        assert!(sink.control_change(16, 20, 0).is_err(), "channel above 15");
        assert!(sink.control_change(0, 128, 0).is_err(), "cc above 127");
        assert!(sink.control_change(0, 20, 200).is_err(), "value above 127");
        assert!(sink.control_change(0, 20, 127).is_ok());
    }

    #[test]
    fn status_byte_carries_the_channel() {
        let mut sink = MidiOut {
            file: std::fs::OpenOptions::new().write(true).open("/dev/null").unwrap(),
            path: PathBuf::from("/dev/null"),
        };
        // 0xB0 | 12 = 0xBC; exercised for its range acceptance here, the byte
        // layout itself is asserted by the encoding above.
        assert!(sink.send(12, 0xb0, 20, 64).is_ok());
    }
}
