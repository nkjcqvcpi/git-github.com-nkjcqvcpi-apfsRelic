#![no_main]
//! Fuzz the B-tree engine over arbitrary 4 KiB blocks: node parsing and
//! omap/fs traversal must never panic or read out of bounds.

use libfuzzer_sys::fuzz_target;

use apfsrelic_core::apfs::btree::BtreeReader;
use apfsrelic_core::device::BlockDevice;
use apfsrelic_core::error::Result;

struct MemDevice(Vec<u8>);
impl BlockDevice for MemDevice {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let o = offset as usize;
        let end = o.checked_add(buf.len()).unwrap_or(usize::MAX);
        if end > self.0.len() {
            return Err(apfsrelic_core::error::Error::new(apfsrelic_core::error::ErrorKind::Io, "oob"));
        }
        buf.copy_from_slice(&self.0[o..end]);
        Ok(())
    }
    fn description(&self) -> &str {
        "fuzz"
    }
}

fuzz_target!(|data: &[u8]| {
    // Pad to at least a few blocks so node reads can succeed structurally.
    let mut v = data.to_vec();
    v.resize(4096 * 4, 0);
    let dev = MemDevice(v);
    let bt = BtreeReader::new(&dev, 4096);
    let _ = bt.omap_get(0, 1, u64::MAX);
    let _ = bt.fs_collect(None, 0, 1, u64::MAX);
    let _ = bt.fs_walk(None, 0, u64::MAX, &mut |_, _| Ok(true));
});
