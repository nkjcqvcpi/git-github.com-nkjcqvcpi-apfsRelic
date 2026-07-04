//! Shared test helpers: an in-memory block device and synthetic APFS B-tree node
//! builders used to exercise the engine without a real container.
//!
//! Each integration test binary includes this module but uses a different subset
//! of the helpers, so unused-item warnings here are expected.
#![allow(dead_code)]

use apfsrelic_core::apfs::checksum;
use apfsrelic_core::device::BlockDevice;
use apfsrelic_core::error::Result;

/// Block size used by the synthetic fixtures.
pub const BS: usize = 4096;
const BTREE_INFO: usize = 40;
const BTN_DATA: usize = 56;

/// A minimal in-memory, read-only block device.
pub struct MemDevice {
    pub data: Vec<u8>,
}

impl MemDevice {
    pub fn new(data: Vec<u8>) -> Self {
        MemDevice { data }
    }
}

impl BlockDevice for MemDevice {
    fn size(&self) -> u64 {
        self.data.len() as u64
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let o = offset as usize;
        let end = o + buf.len();
        if end > self.data.len() {
            return Err(apfsrelic_core::error::Error::new(
                apfsrelic_core::error::ErrorKind::Io,
                "mem read past end",
            ));
        }
        buf.copy_from_slice(&self.data[o..end]);
        Ok(())
    }
    fn description(&self) -> &str {
        "mem"
    }
}

fn put_u16(b: &mut [u8], off: usize, v: u16) {
    b[off..off + 2].copy_from_slice(&v.to_le_bytes());
}
fn put_u32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
fn put_u64(b: &mut [u8], off: usize, v: u64) {
    b[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

fn stamp(b: &mut [u8]) {
    let ck = checksum::compute(b);
    b[0..8].copy_from_slice(&ck.to_le_bytes());
}

/// Build a single root+leaf **object-map** node (fixed kv) from `(oid, xid,
/// paddr)` entries, which must be sorted by `(oid, xid)`.
pub fn build_omap_root_leaf(entries: &[(u64, u64, u64)]) -> Vec<u8> {
    let mut b = vec![0u8; BS];
    put_u32(&mut b, 24, 0x4000_0002); // o_type: physical btree
    put_u16(&mut b, 32, 0x0007); // flags: root|leaf|fixed
    put_u16(&mut b, 34, 0); // level
    let n = entries.len() as u32;
    put_u32(&mut b, 36, n); // nkeys
    put_u16(&mut b, 40, 0); // table_space.off
    put_u16(&mut b, 42, (n * 4) as u16); // table_space.len (kvoff = 4 bytes)

    let toc = BTN_DATA;
    let key_area = BTN_DATA + n as usize * 4;
    let val_end = BS - BTREE_INFO;

    for (i, &(oid, xid, paddr)) in entries.iter().enumerate() {
        put_u16(&mut b, toc + i * 4, (i * 16) as u16); // k
        put_u16(&mut b, toc + i * 4 + 2, ((i + 1) * 16) as u16); // v
        let kpos = key_area + i * 16;
        put_u64(&mut b, kpos, oid);
        put_u64(&mut b, kpos + 8, xid);
        let vpos = val_end - (i + 1) * 16;
        put_u32(&mut b, vpos, 0); // omap_val.flags
        put_u32(&mut b, vpos + 4, BS as u32); // omap_val.size
        put_u64(&mut b, vpos + 8, paddr); // omap_val.paddr
    }
    stamp(&mut b);
    b
}

/// Build a single root+leaf **filesystem** node (variable kv) from `(key, val)`
/// records, which must be sorted by the APFS key order.
pub fn build_fs_root_leaf(records: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut b = vec![0u8; BS];
    put_u32(&mut b, 24, 0x0000_000e); // o_type: virtual fstree
    put_u16(&mut b, 32, 0x0003); // flags: root|leaf (variable kv)
    put_u16(&mut b, 34, 0);
    let n = records.len() as u32;
    put_u32(&mut b, 36, n);
    put_u16(&mut b, 40, 0); // table_space.off
    put_u16(&mut b, 42, (n * 8) as u16); // table_space.len (kvloc = 8 bytes)

    let toc = BTN_DATA;
    let key_area = BTN_DATA + n as usize * 8;
    let val_end = BS - BTREE_INFO;

    let mut key_cursor = 0usize;
    let mut val_cursor = 0usize; // cumulative value bytes from val_end
    for (i, (key, val)) in records.iter().enumerate() {
        // key
        let koff = key_cursor;
        b[key_area + koff..key_area + koff + key.len()].copy_from_slice(key);
        // value (placed back from val_end)
        val_cursor += val.len();
        let vpos = val_end - val_cursor;
        b[vpos..vpos + val.len()].copy_from_slice(val);

        // kvloc TOC entry: k.off, k.len, v.off, v.len
        let t = toc + i * 8;
        put_u16(&mut b, t, koff as u16);
        put_u16(&mut b, t + 2, key.len() as u16);
        put_u16(&mut b, t + 4, val_cursor as u16); // v.off = cumulative from end
        put_u16(&mut b, t + 6, val.len() as u16);

        key_cursor += key.len();
    }
    stamp(&mut b);
    b
}

/// Compose `obj_id_and_type` from an object id and a type nibble.
pub fn obj_id_and_type(oid: u64, ty: u8) -> u64 {
    (oid & 0x0fff_ffff_ffff_ffff) | ((ty as u64) << 60)
}
