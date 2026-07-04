//! `OffsetDevice` — a sub-window of another [`BlockDevice`] starting at a fixed
//! byte offset. Used to present an APFS container that begins at a partition
//! offset (or any user-supplied `--offset`) as if it began at byte 0.

use std::sync::Arc;

use super::{check_range, BlockDevice};
use crate::error::Result;

/// A read-only view of `[base, base+len)` of an inner device, re-based to 0.
pub struct OffsetDevice {
    inner: Arc<dyn BlockDevice>,
    base: u64,
    len: u64,
    description: String,
}

impl OffsetDevice {
    /// Window `inner` to `len` bytes starting at byte `base`.
    pub fn new(inner: Arc<dyn BlockDevice>, base: u64, len: u64) -> Self {
        let description = format!("{} @ offset {} ({} bytes)", inner.description(), base, len);
        OffsetDevice {
            inner,
            base,
            len,
            description,
        }
    }
}

impl BlockDevice for OffsetDevice {
    fn size(&self) -> u64 {
        self.len
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if buf.is_empty() {
            return Ok(());
        }
        check_range(offset, buf.len(), self.len)?;
        self.inner.read_at(self.base + offset, buf)
    }

    fn description(&self) -> &str {
        &self.description
    }
}
