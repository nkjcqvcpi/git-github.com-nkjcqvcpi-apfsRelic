//! GPT partition discovery and the `PartitionedImageDevice` window (rewrite plan
//! Phase 3).
//!
//! A whole-disk image (or the virtual disk inside a `.sparsebundle`) typically
//! carries a GUID Partition Table; the APFS container lives inside an
//! `Apple_APFS` partition rather than at byte 0. This module parses the GPT,
//! lists partitions, finds the APFS one, and produces an [`OffsetDevice`] window
//! onto the chosen partition. GPT addressing uses a fixed 512-byte LBA, matching
//! the C prototype and the on-disk reality of Apple images.

use std::sync::Arc;

use super::offset::OffsetDevice;
use super::BlockDevice;
use crate::error::{Error, ErrorKind, Result};

/// LBA size used for GPT addressing.
pub const GPT_LBA_SIZE: u64 = 512;

/// On-disk (mixed-endian) bytes of the Apple_APFS partition type GUID
/// `7C3457EF-0000-11AA-AA11-00306543ECAC`.
const APFS_TYPE_GUID: [u8; 16] = [
    0xEF, 0x57, 0x34, 0x7C, 0x00, 0x00, 0xAA, 0x11, 0xAA, 0x11, 0x00, 0x30, 0x65, 0x43, 0xEC, 0xAC,
];

/// One GPT partition entry, decoded.
#[derive(Debug, Clone)]
pub struct Partition {
    /// 1-based index as presented to the user.
    pub index: u32,
    /// Partition type GUID in canonical text form.
    pub type_guid: String,
    /// Unique partition GUID in canonical text form.
    pub unique_guid: String,
    pub first_lba: u64,
    pub last_lba: u64,
    /// UTF-16LE partition name, trimmed of trailing NULs.
    pub name: String,
    /// True if the type GUID is `Apple_APFS`.
    pub is_apfs: bool,
}

impl Partition {
    /// Byte offset of the partition within the image.
    pub fn byte_offset(&self) -> u64 {
        self.first_lba * GPT_LBA_SIZE
    }

    /// Byte length of the partition (inclusive LBA range).
    pub fn byte_len(&self) -> u64 {
        if self.last_lba >= self.first_lba {
            (self.last_lba - self.first_lba + 1) * GPT_LBA_SIZE
        } else {
            0
        }
    }
}

/// A parsed GUID Partition Table.
#[derive(Debug, Clone)]
pub struct PartitionTable {
    pub entries: Vec<Partition>,
}

impl PartitionTable {
    /// Parse the GPT of `dev`. Returns `Ok(None)` if there is no GPT (the image
    /// is a bare APFS container), `Err` only on read failure of LBA 1.
    pub fn parse(dev: &dyn BlockDevice) -> Result<Option<PartitionTable>> {
        // GPT header lives at LBA 1.
        if dev.size() < 2 * GPT_LBA_SIZE {
            return Ok(None);
        }
        let header = dev.read_vec(GPT_LBA_SIZE, GPT_LBA_SIZE as usize)?;
        if &header[0..8] != b"EFI PART" {
            return Ok(None);
        }

        let entry_lba = read_u64(&header, 72);
        let num_entries = read_u32(&header, 80);
        let entry_size = read_u32(&header, 84);

        // Reject implausible geometry rather than trusting the on-disk counts.
        if !(128..=4096).contains(&entry_size) || num_entries == 0 || num_entries > 1024 {
            return Err(Error::new(
                ErrorKind::Corrupt,
                format!("GPT geometry invalid (count={num_entries}, entry_size={entry_size})"),
            ));
        }

        let mut entries = Vec::new();
        for i in 0..num_entries {
            let off = entry_lba * GPT_LBA_SIZE + i as u64 * entry_size as u64;
            // Some images declare more entries than the device actually holds;
            // stop cleanly at the first unreadable entry.
            let entry = match dev.read_vec(off, entry_size as usize) {
                Ok(e) => e,
                Err(_) => break,
            };
            // All-zero type GUID => empty slot.
            if entry[0..16].iter().all(|&b| b == 0) {
                continue;
            }
            let is_apfs = entry[0..16] == APFS_TYPE_GUID;
            entries.push(Partition {
                index: i + 1,
                type_guid: format_guid(&entry[0..16]),
                unique_guid: format_guid(&entry[16..32]),
                first_lba: read_u64(&entry, 32),
                last_lba: read_u64(&entry, 40),
                name: decode_utf16le_name(&entry[56..entry_size.min(128) as usize]),
                is_apfs,
            });
        }

        Ok(Some(PartitionTable { entries }))
    }

    /// All APFS partitions, in table order.
    pub fn apfs_partitions(&self) -> Vec<&Partition> {
        self.entries.iter().filter(|p| p.is_apfs).collect()
    }
}

/// Resolve the APFS container offset for an image.
///
/// `explicit_offset` (from `--offset`) wins; otherwise `selector` chooses among
/// GPT partitions. Returns `(offset, len)` of the container window, where `len`
/// is the device size minus offset when the partition length is unknown.
pub fn resolve_container_window(
    dev: &dyn BlockDevice,
    selector: PartitionSelector,
    explicit_offset: Option<u64>,
) -> Result<(u64, u64, Option<u32>)> {
    if let Some(off) = explicit_offset {
        if off >= dev.size() {
            return Err(Error::new(
                ErrorKind::Usage,
                format!("--offset {off} is past the image size {}", dev.size()),
            ));
        }
        return Ok((off, dev.size() - off, None));
    }

    let table = PartitionTable::parse(dev)?;
    let table = match table {
        // No GPT: bare container at offset 0.
        None => return Ok((0, dev.size(), None)),
        Some(t) => t,
    };

    match selector {
        PartitionSelector::Auto => {
            let apfs = table.apfs_partitions();
            match apfs.len() {
                0 => Err(Error::new(
                    ErrorKind::UnsupportedFormat,
                    "no Apple_APFS partition found in GPT",
                )),
                1 => {
                    let p = apfs[0];
                    Ok((p.byte_offset(), window_len(dev, p), Some(p.index)))
                }
                _ => Err(Error::new(
                    ErrorKind::Usage,
                    format!(
                        "image has {} APFS partitions; choose one with --partition <index>",
                        apfs.len()
                    ),
                )),
            }
        }
        PartitionSelector::Index(idx) => {
            let p = table
                .entries
                .iter()
                .find(|p| p.index == idx)
                .ok_or_else(|| {
                    Error::new(ErrorKind::Usage, format!("no partition with index {idx}"))
                })?;
            Ok((p.byte_offset(), window_len(dev, p), Some(p.index)))
        }
    }
}

fn window_len(dev: &dyn BlockDevice, p: &Partition) -> u64 {
    let by_len = p.byte_len();
    let by_dev = dev.size().saturating_sub(p.byte_offset());
    if by_len == 0 || by_len > by_dev {
        by_dev
    } else {
        by_len
    }
}

/// How to choose a partition when no explicit `--offset` is given.
#[derive(Debug, Clone, Copy)]
pub enum PartitionSelector {
    /// Auto-detect the single APFS partition (error if ambiguous).
    Auto,
    /// A specific 1-based GPT index.
    Index(u32),
}

/// A read-only window onto a partition (or `--offset` region) of an image.
///
/// This is a thin newtype over [`OffsetDevice`]; it exists so call sites can
/// name the partitioned case explicitly (rewrite plan Phase 1's backend list).
pub struct PartitionedImageDevice {
    inner: OffsetDevice,
}

impl PartitionedImageDevice {
    pub fn new(image: Arc<dyn BlockDevice>, offset: u64, len: u64) -> Self {
        PartitionedImageDevice {
            inner: OffsetDevice::new(image, offset, len),
        }
    }
}

impl BlockDevice for PartitionedImageDevice {
    fn size(&self) -> u64 {
        self.inner.size()
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.inner.read_at(offset, buf)
    }
    fn description(&self) -> &str {
        self.inner.description()
    }
}

fn read_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}
fn read_u64(b: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(a)
}

/// Format 16 raw GUID bytes (mixed-endian on-disk layout) as canonical text.
fn format_guid(b: &[u8]) -> String {
    // Data1/2/3 are little-endian; Data4 (last 8 bytes) is big-endian.
    let d1 = read_u32(b, 0);
    let d2 = u16::from_le_bytes([b[4], b[5]]);
    let d3 = u16::from_le_bytes([b[6], b[7]]);
    format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        d1, d2, d3, b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// Decode a UTF-16LE partition name field, stopping at the first NUL pair.
fn decode_utf16le_name(b: &[u8]) -> String {
    let mut units = Vec::new();
    let mut i = 0;
    while i + 1 < b.len() {
        let u = u16::from_le_bytes([b[i], b[i + 1]]);
        if u == 0 {
            break;
        }
        units.push(u);
        i += 2;
    }
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apfs_guid_formats_canonically() {
        assert_eq!(
            format_guid(&APFS_TYPE_GUID),
            "7C3457EF-0000-11AA-AA11-00306543ECAC"
        );
    }
}
