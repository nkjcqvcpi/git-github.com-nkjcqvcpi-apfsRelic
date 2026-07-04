//! The central read-only block-device abstraction (rewrite plan Phase 1).
//!
//! Every APFS parser reads through [`BlockDevice`]. No APFS code opens files,
//! seeks, or knows whether the source is a sparsebundle, a raw image, a DMG, or
//! a device path. Backends are read-only by construction: the trait exposes no
//! write method and the concrete backends open their files without write access.

use crate::error::{Error, ErrorKind, Result};

pub mod offset;
pub mod partition;
pub mod raw;
pub mod sparsebundle;

pub use offset::OffsetDevice;
pub use partition::{Partition, PartitionTable, PartitionedImageDevice};
pub use raw::RawFileDevice;
pub use sparsebundle::SparseBundleDevice;

/// A read-only random-access block device backing an APFS container.
///
/// Implementors must be safe to call from a single thread; the engine clones
/// cheap handles or wraps the device in an `Rc`/`Arc` as needed. Reads past the
/// end of the logical device are an error, not a panic.
pub trait BlockDevice: Send + Sync {
    /// Logical size of the device in bytes.
    fn size(&self) -> u64;

    /// Read exactly `buf.len()` bytes starting at byte `offset`.
    ///
    /// Returns [`ErrorKind::Io`] if the read would extend past the end of the
    /// device or the backend fails. Sparse/unmapped regions read as zeros where
    /// the backend's semantics allow it (e.g. absent sparsebundle bands).
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()>;

    /// A short human description of the source (shown in `inspect`/`--json`).
    fn description(&self) -> &str;

    /// Read `len` bytes at `offset` into a fresh `Vec`.
    fn read_vec(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        self.read_at(offset, &mut buf)?;
        Ok(buf)
    }

    /// Read APFS block number `block` given `block_size`. Blocks are
    /// `block_size`-byte units numbered from 0.
    fn read_block(&self, block: u64, block_size: u32) -> Result<Vec<u8>> {
        let offset = block
            .checked_mul(block_size as u64)
            .ok_or_else(|| Error::new(ErrorKind::Corrupt, "block address overflow"))?;
        self.read_vec(offset, block_size as usize)
    }
}

/// Helper for backends to validate a requested range against the device size.
pub(crate) fn check_range(offset: u64, len: usize, size: u64) -> Result<()> {
    let end = offset
        .checked_add(len as u64)
        .ok_or_else(|| Error::new(ErrorKind::Io, "read range overflows u64"))?;
    if end > size {
        return Err(Error::new(
            ErrorKind::Io,
            format!("read of {len} bytes at offset {offset} extends past device size {size}"),
        ));
    }
    Ok(())
}
