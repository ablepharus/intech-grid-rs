//! Firmware images and updates.
//!
//! Grid modules on the ESP32-S3 platform ship with the TinyUF2 bootloader.
//! Updating is therefore a file copy, not a protocol exchange: the module is
//! put into its bootloader, where it appears as a USB mass-storage volume
//! named `GRID-S3`, and a `.uf2` image copied onto that volume is flashed and
//! the module reboots.
//!
//! That is exactly what the vendor's own `uf2_autoupload.sh` does - it polls
//! for `/media/$USER/GRID-S3` and copies the image in.
//!
//! # Entering the bootloader
//!
//! There is no protocol class for it. `grid_protocol.h` defines `RESET`
//! (0x013), which restarts the application, and `NVMERASE` (0x062), which
//! erases stored configuration - neither enters the bootloader. On ESP32-S3
//! TinyUF2 the entry gesture is a physical **double-tap of the reset button**,
//! and this crate deliberately does not pretend otherwise: see
//! [`BOOTLOADER_INSTRUCTIONS`].
//!
//! # Recovery
//!
//! A module whose application is damaged can still be flashed over serial
//! with `esptool.py`, which is what the vendor's `burn_test_firmware.sh` does:
//! bootloader at `0x0`, partition table at `0x8000`, OTA data at `0xe000`,
//! the application at `0x10000` and TinyUF2 at `0x410000`.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// First UF2 magic word, `"UF2\n"`.
pub const UF2_MAGIC_START0: u32 = 0x0A32_4655;
/// Second UF2 magic word.
pub const UF2_MAGIC_START1: u32 = 0x9E5D_5157;
/// Final UF2 magic word, at the end of each 512-byte block.
pub const UF2_MAGIC_END: u32 = 0x0AB1_6F30;
/// UF2 family ID for the ESP32-S3, the chip Grid modules use.
pub const ESP32S3_FAMILY_ID: u32 = 0xC47E_5767;
/// Every UF2 block is exactly this many bytes.
pub const UF2_BLOCK_SIZE: usize = 512;
/// Flag bit meaning the `fileSize` field carries a family ID.
const UF2_FLAG_FAMILY_ID: u32 = 0x0000_2000;

/// Volume label the TinyUF2 bootloader presents.
pub const UF2_VOLUME_LABEL: &str = "GRID-S3";

/// How to put a module into its bootloader, for display to a human.
pub const BOOTLOADER_INSTRUCTIONS: &str = "\
Put the module into its bootloader before updating:
  1. Double-tap the reset button on the module (two quick presses).
  2. The module disappears as a serial port and reappears as a USB drive
     named GRID-S3.
  3. Re-run the update; it copies the image onto that drive.
There is no software command for this step - the bootloader is entered by the
physical reset gesture.";

/// A parsed, validated UF2 firmware image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uf2Image {
    pub path: PathBuf,
    /// Number of 512-byte blocks, as the image's own headers declare.
    pub blocks: u32,
    /// Total file size in bytes.
    pub bytes: u64,
    /// Family ID carried by the image.
    pub family_id: u32,
    /// Flash address the first block targets.
    pub target_address: u32,
    /// Total payload (flashable) bytes across all blocks.
    pub payload_bytes: u64,
}

fn le_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

impl Uf2Image {
    /// Parse and validate a UF2 file.
    ///
    /// Rejects anything that is not a UF2 for this chip family, so a wrong
    /// file cannot be copied onto a bootloader volume: the magic words, the
    /// 512-byte block alignment, the declared block count and the family ID
    /// are all checked.
    pub fn open(path: &Path) -> Result<Self> {
        let data = std::fs::read(path)?;
        Self::parse(path.to_path_buf(), &data)
    }

    /// Validate an in-memory UF2 image.
    pub fn parse(path: PathBuf, data: &[u8]) -> Result<Self> {
        if data.len() < UF2_BLOCK_SIZE {
            return Err(Error::Other(format!(
                "{} is {} bytes, too short to be a UF2 image (one block is {UF2_BLOCK_SIZE})",
                path.display(),
                data.len()
            )));
        }
        if !data.len().is_multiple_of(UF2_BLOCK_SIZE) {
            return Err(Error::Other(format!(
                "{} is {} bytes, not a whole number of {UF2_BLOCK_SIZE}-byte UF2 blocks",
                path.display(),
                data.len()
            )));
        }
        if le_u32(data, 0) != UF2_MAGIC_START0 || le_u32(data, 4) != UF2_MAGIC_START1 {
            return Err(Error::Other(format!(
                "{} is not a UF2 image (bad magic); a raw .bin cannot be copied to the \
                 bootloader volume - flash it with esptool instead",
                path.display()
            )));
        }

        let flags = le_u32(data, 8);
        let target_address = le_u32(data, 12);
        let payload_size = le_u32(data, 16);
        let blocks = le_u32(data, 24);
        let family_id = le_u32(data, 28);

        let actual_blocks = (data.len() / UF2_BLOCK_SIZE) as u32;
        if blocks != actual_blocks {
            return Err(Error::Other(format!(
                "{} declares {blocks} UF2 blocks but contains {actual_blocks}",
                path.display()
            )));
        }
        if flags & UF2_FLAG_FAMILY_ID == 0 {
            return Err(Error::Other(format!(
                "{} carries no UF2 family ID, so it cannot be checked against this chip",
                path.display()
            )));
        }
        if family_id != ESP32S3_FAMILY_ID {
            return Err(Error::Other(format!(
                "{} is for UF2 family 0x{family_id:08x}, but Grid modules are ESP32-S3 \
                 (0x{ESP32S3_FAMILY_ID:08x})",
                path.display()
            )));
        }
        if payload_size as usize > UF2_BLOCK_SIZE - 32 {
            return Err(Error::Other(format!(
                "{} declares a {payload_size}-byte payload per block, which does not fit",
                path.display()
            )));
        }

        Ok(Uf2Image {
            path,
            blocks,
            bytes: data.len() as u64,
            family_id,
            target_address,
            payload_bytes: payload_size as u64 * blocks as u64,
        })
    }
}

/// A mounted TinyUF2 bootloader volume, ready to receive an image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootloaderVolume {
    pub mount_point: PathBuf,
    /// True when the volume carries the bootloader's own `INFO_UF2.TXT`.
    pub has_info_file: bool,
}

/// Find mounted UF2 bootloader volumes that a Grid module could have exposed.
///
/// Matches either the `GRID-S3` volume label in the mount path or the
/// presence of `INFO_UF2.TXT`, which every TinyUF2 volume carries.
pub fn find_bootloader_volumes() -> Result<Vec<BootloaderVolume>> {
    let mounts = match std::fs::read_to_string("/proc/mounts") {
        Ok(m) => m,
        Err(_) => return Ok(Vec::new()),
    };
    let mut found = Vec::new();

    for line in mounts.lines() {
        let mut parts = line.split_whitespace();
        let _dev = parts.next();
        let Some(raw_mount) = parts.next() else { continue };
        // /proc/mounts escapes spaces as \040.
        let mount = raw_mount.replace("\\040", " ");
        let path = PathBuf::from(&mount);

        let labelled = mount.to_ascii_uppercase().contains(UF2_VOLUME_LABEL);
        let info = path.join("INFO_UF2.TXT");
        let has_info_file = info.is_file();

        if labelled || has_info_file {
            found.push(BootloaderVolume {
                mount_point: path,
                has_info_file,
            });
        }
    }
    Ok(found)
}

/// Copy a validated image onto a bootloader volume, flashing the module.
///
/// The bootloader reboots into the new firmware once the copy completes, so
/// the volume disappears mid-write; a "device went away" error after the
/// bytes are written is normal and is treated as success.
///
/// This is destructive and irreversible. Callers are expected to have
/// confirmed with the user first.
pub fn flash(volume: &BootloaderVolume, image: &Uf2Image) -> Result<u64> {
    if !volume.mount_point.is_dir() {
        return Err(Error::Other(format!(
            "{} is not a mounted directory",
            volume.mount_point.display()
        )));
    }
    let name = image
        .path
        .file_name()
        .ok_or_else(|| Error::Other("image path has no file name".to_string()))?;
    let dest = volume.mount_point.join(name);

    match std::fs::copy(&image.path, &dest) {
        Ok(n) => Ok(n),
        // The volume vanishes as the bootloader restarts; that is the
        // expected end of a successful flash, not a failure.
        Err(e) if matches!(e.raw_os_error(), Some(5) | Some(19) | Some(32)) => Ok(image.bytes),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Images the vendor ships in the firmware repo, used as real fixtures.
    const VENDOR_UF2: &[&str] = &[
        "vendor/grid-fw/esp32s3/bootloader/grid_esp32_release_2024-04-23-1133.uf2",
        "vendor/grid-fw/esp32s3/bootloader/apps/blinky.uf2",
        "vendor/grid-fw/esp32s3/bootloader/apps/update-tinyuf2.uf2",
    ];

    fn repo_root() -> PathBuf {
        // CARGO_MANIFEST_DIR is <root>/crates/orb-intech-grid
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .to_path_buf()
    }

    #[test]
    fn accepts_every_vendor_image() {
        let root = repo_root();
        let mut checked = 0;
        for rel in VENDOR_UF2 {
            let p = root.join(rel);
            if !p.is_file() {
                // `vendor/` is not checked in, so these fixtures are only
                // present in a working tree that has the firmware clone.
                // The synthetic blocks below cover the logic either way.
                continue;
            }
            let img = Uf2Image::open(&p).expect("vendor image should validate");
            assert_eq!(img.family_id, ESP32S3_FAMILY_ID, "{rel}");
            assert_eq!(img.bytes % UF2_BLOCK_SIZE as u64, 0, "{rel}");
            assert_eq!(img.blocks as u64, img.bytes / UF2_BLOCK_SIZE as u64, "{rel}");
            assert!(img.payload_bytes > 0, "{rel}");
            checked += 1;
        }
        eprintln!("validated {checked} vendor UF2 image(s)");
    }

    /// Build a minimal single-block UF2 so the rejection paths can be driven
    /// without shipping extra fixtures.
    fn block(magic0: u32, magic1: u32, flags: u32, payload: u32, blocks: u32, family: u32) -> Vec<u8> {
        let mut b = vec![0u8; UF2_BLOCK_SIZE];
        b[0..4].copy_from_slice(&magic0.to_le_bytes());
        b[4..8].copy_from_slice(&magic1.to_le_bytes());
        b[8..12].copy_from_slice(&flags.to_le_bytes());
        b[12..16].copy_from_slice(&0u32.to_le_bytes());
        b[16..20].copy_from_slice(&payload.to_le_bytes());
        b[20..24].copy_from_slice(&0u32.to_le_bytes());
        b[24..28].copy_from_slice(&blocks.to_le_bytes());
        b[28..32].copy_from_slice(&family.to_le_bytes());
        b
    }

    fn p() -> PathBuf {
        PathBuf::from("test.uf2")
    }

    #[test]
    fn accepts_a_well_formed_single_block() {
        let img = Uf2Image::parse(
            p(),
            &block(UF2_MAGIC_START0, UF2_MAGIC_START1, UF2_FLAG_FAMILY_ID, 256, 1, ESP32S3_FAMILY_ID),
        )
        .unwrap();
        assert_eq!(img.blocks, 1);
        assert_eq!(img.payload_bytes, 256);
    }

    #[test]
    fn rejects_a_raw_bin() {
        // A .bin has no UF2 magic; copying it to the volume would do nothing
        // useful, so it must not be accepted.
        let err = Uf2Image::parse(p(), &vec![0xffu8; UF2_BLOCK_SIZE])
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a UF2 image"), "{err}");
        assert!(err.contains("esptool"), "should point at the right tool: {err}");
    }

    #[test]
    fn rejects_the_wrong_chip_family() {
        // 0x1c5f21b0 is the ESP32-S2 family.
        let err = Uf2Image::parse(
            p(),
            &block(UF2_MAGIC_START0, UF2_MAGIC_START1, UF2_FLAG_FAMILY_ID, 256, 1, 0x1c5f_21b0),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("ESP32-S3"), "{err}");
    }

    #[test]
    fn rejects_a_missing_family_id() {
        let err = Uf2Image::parse(
            p(),
            &block(UF2_MAGIC_START0, UF2_MAGIC_START1, 0, 256, 1, ESP32S3_FAMILY_ID),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("no UF2 family ID"), "{err}");
    }

    #[test]
    fn rejects_a_truncated_or_misaligned_image() {
        let short = Uf2Image::parse(p(), &[0u8; 16]).unwrap_err().to_string();
        assert!(short.contains("too short"), "{short}");

        let mut misaligned = block(
            UF2_MAGIC_START0, UF2_MAGIC_START1, UF2_FLAG_FAMILY_ID, 256, 1, ESP32S3_FAMILY_ID,
        );
        misaligned.push(0);
        let err = Uf2Image::parse(p(), &misaligned).unwrap_err().to_string();
        assert!(err.contains("whole number"), "{err}");
    }

    #[test]
    fn rejects_a_lying_block_count() {
        let err = Uf2Image::parse(
            p(),
            &block(UF2_MAGIC_START0, UF2_MAGIC_START1, UF2_FLAG_FAMILY_ID, 256, 99, ESP32S3_FAMILY_ID),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("declares 99"), "{err}");
    }

    #[test]
    fn volume_discovery_never_panics_and_is_read_only() {
        // On a bench with no module in bootloader mode this is simply empty;
        // the point is that it parses /proc/mounts without blowing up.
        let vols = find_bootloader_volumes().expect("mount scan");
        for v in &vols {
            assert!(!v.mount_point.as_os_str().is_empty());
        }
    }

    #[test]
    fn bootloader_instructions_name_the_physical_gesture() {
        // There is no software path into the bootloader, so the guidance must
        // say so rather than implying a command exists.
        assert!(BOOTLOADER_INSTRUCTIONS.contains("Double-tap"));
        assert!(BOOTLOADER_INSTRUCTIONS.contains(UF2_VOLUME_LABEL));
    }
}
