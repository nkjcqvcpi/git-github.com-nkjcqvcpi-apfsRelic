//! File-data extraction engine: turn a set of file extents into a byte stream
//! following the logical file layout (rewrite plan Phase 16). Separated from the
//! `recover` command so the byte-exact layout logic is unit-testable without a
//! CLI or a real container.

use std::io::Write;

use super::jrec::FileExtent;
use crate::device::BlockDevice;
use crate::error::Result;

/// Outcome of writing file bytes.
#[derive(Debug, Default, Clone, Copy)]
pub struct Written {
    /// Total bytes written (should equal the logical file size on success).
    pub bytes: u64,
    /// Number of zero-filled hole/gap regions.
    pub holes: u64,
    /// Number of overlapping extents skipped.
    pub overlaps: u64,
    /// Bytes that could not be written (file size minus bytes written).
    pub missing: u64,
}

impl Written {
    /// A short human note describing anomalies, or `None` if clean.
    pub fn note(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.holes > 0 {
            parts.push(format!("{} hole(s)", self.holes));
        }
        if self.overlaps > 0 {
            parts.push(format!("{} overlap(s)", self.overlaps));
        }
        if self.missing > 0 {
            parts.push(format!("{} missing byte(s)", self.missing));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    }
}

/// Write `file_size` bytes to `writer` following the logical layout of
/// `extents`. Extents are sorted by logical address; gaps and zero-block extents
/// become zero-filled holes; overlapping extents keep the earlier data; output
/// stops exactly at `file_size`. Works sequentially so it is correct for both a
/// seekable file and a non-seekable stream (e.g. stdout).
pub fn write_extents(
    dev: &dyn BlockDevice,
    block_size: u32,
    extents: &mut [FileExtent],
    file_size: u64,
    writer: &mut dyn Write,
) -> Result<Written> {
    extents.sort_by_key(|e| e.logical_addr);

    let bs = block_size as u64;
    let zero = vec![0u8; block_size as usize];
    let mut cursor = 0u64;
    let mut holes = 0u64;
    let mut overlaps = 0u64;

    for e in extents.iter() {
        if cursor >= file_size {
            break;
        }
        if e.logical_addr > cursor {
            // Gap before this extent => sparse hole.
            let gap = (e.logical_addr - cursor).min(file_size - cursor);
            write_zeros(writer, gap, &zero)?;
            holes += 1;
            cursor += gap;
        } else if e.logical_addr < cursor {
            // Overlapping extent: keep the earlier data already written.
            overlaps += 1;
            continue;
        }
        if cursor >= file_size {
            break;
        }

        let want = e.len.min(file_size - cursor);
        if e.is_hole() {
            write_zeros(writer, want, &zero)?;
            holes += 1;
            cursor += want;
            continue;
        }

        let mut block = e.phys_block_num;
        let mut remaining = want;
        while remaining > 0 {
            let data = dev.read_block(block, block_size)?;
            let n = remaining.min(bs);
            writer.write_all(&data[..n as usize])?;
            remaining -= n;
            cursor += n;
            block += 1;
        }
    }

    if cursor < file_size {
        write_zeros(writer, file_size - cursor, &zero)?;
        holes += 1;
        cursor = file_size;
    }

    Ok(Written {
        bytes: cursor,
        holes,
        overlaps,
        missing: file_size.saturating_sub(cursor),
    })
}

fn write_zeros(writer: &mut dyn Write, mut n: u64, zero: &[u8]) -> Result<()> {
    while n > 0 {
        let chunk = (n as usize).min(zero.len());
        writer.write_all(&zero[..chunk])?;
        n -= chunk as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result as DResult;

    /// Minimal in-memory block device for tests.
    struct MemDevice {
        data: Vec<u8>,
    }
    impl BlockDevice for MemDevice {
        fn size(&self) -> u64 {
            self.data.len() as u64
        }
        fn read_at(&self, offset: u64, buf: &mut [u8]) -> DResult<()> {
            let o = offset as usize;
            buf.copy_from_slice(&self.data[o..o + buf.len()]);
            Ok(())
        }
        fn description(&self) -> &str {
            "mem"
        }
    }

    fn extent(logical: u64, len: u64, phys: u64) -> FileExtent {
        FileExtent {
            logical_addr: logical,
            len,
            phys_block_num: phys,
            crypto_id: 0,
        }
    }

    #[test]
    fn writes_sparse_file_with_hole() {
        // 3 blocks of 4 bytes. Block 1 = "AAAA", block 2 = "BBBB".
        let bs = 4u32;
        let mut data = vec![0u8; 12];
        data[4..8].copy_from_slice(b"AAAA"); // block 1
        data[8..12].copy_from_slice(b"BBBB"); // block 2
        let dev = MemDevice { data };

        // File: [0,4) from block 1, [4,8) hole, [8,12) from block 2. size=12.
        let mut extents = vec![
            extent(0, 4, 1),
            // logical 8 from block 2; logical 4..8 is a gap (hole).
            extent(8, 4, 2),
        ];
        let mut out = Vec::new();
        let w = write_extents(&dev, bs, &mut extents, 12, &mut out).unwrap();
        assert_eq!(out, b"AAAA\0\0\0\0BBBB");
        assert_eq!(w.bytes, 12);
        assert!(w.holes >= 1);
        assert_eq!(w.missing, 0);
    }

    #[test]
    fn explicit_hole_extent_zero_fills() {
        let bs = 4u32;
        let dev = MemDevice {
            data: b"WXYZ".to_vec(),
        };
        // phys_block_num 0 == hole.
        let mut extents = vec![extent(0, 4, 0)];
        let mut out = Vec::new();
        let w = write_extents(&dev, bs, &mut extents, 4, &mut out).unwrap();
        assert_eq!(out, b"\0\0\0\0");
        assert_eq!(w.bytes, 4);
    }

    #[test]
    fn stops_at_logical_size() {
        let bs = 4u32;
        let dev = MemDevice {
            data: b"ABCD".to_vec(),
        };
        // Extent maps 4 bytes from block 0, but the file size is only 2.
        let mut extents = vec![extent(0, 4, 0)];
        let mut out = Vec::new();
        let w = write_extents(&dev, bs, &mut extents, 2, &mut out).unwrap();
        assert_eq!(out, b"\0\0"); // block 0 is a hole (phys 0); truncated to 2
        assert_eq!(w.bytes, 2);
    }
}
