//! Fuzz-smoke: throw many pseudo-random and edge-case byte buffers at every
//! parser and assert none panics (rewrite plan Phases 24-25). This runs in
//! normal CI; the `fuzz/` directory holds the deeper `cargo-fuzz` harness.

mod common;

use apfsrelic_core::apfs::btree::BtreeReader;
use apfsrelic_core::apfs::jrec::{DirRec, FileExtent, Inode, SnapMetadata, SnapName, Xattr};
use apfsrelic_core::apfs::nx::{CheckpointMap, NxSuperblock};
use apfsrelic_core::apfs::obj::ObjPhys;
use apfsrelic_core::apfs::omap::{OmapKey, OmapPhys, OmapVal};
use apfsrelic_core::apfs::volume::ApfsSuperblock;
use apfsrelic_core::apfs::xfield::parse_xfields;
use common::MemDevice;

/// A tiny deterministic LCG so the test is reproducible (no RNG dependency).
struct Lcg(u64);
impl Lcg {
    fn next_u8(&mut self) -> u8 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u8
    }
    fn fill(&mut self, buf: &mut [u8]) {
        for b in buf.iter_mut() {
            *b = self.next_u8();
        }
    }
}

/// Call every byte-slice parser; none may panic.
fn hammer_parsers(buf: &[u8]) {
    let _ = ObjPhys::parse(buf);
    let _ = NxSuperblock::parse(buf);
    let _ = CheckpointMap::parse(buf);
    let _ = OmapPhys::parse(buf);
    let _ = OmapKey::parse(buf);
    let _ = OmapVal::parse(buf);
    let _ = ApfsSuperblock::parse(buf);
    let _ = parse_xfields(buf);
    let _ = Inode::parse(buf);
    let _ = SnapMetadata::parse(buf);
    // Key/value parsers take two slices; reuse the buffer split in half.
    let mid = buf.len() / 2;
    let (k, v) = buf.split_at(mid);
    let _ = DirRec::parse(k, v);
    let _ = FileExtent::parse(k, v);
    let _ = Xattr::parse(k, v);
    let _ = SnapName::parse(k, v);
}

#[test]
fn parsers_never_panic_on_arbitrary_bytes() {
    let mut rng = Lcg(0x1234_5678_9abc_def0);

    // Edge sizes around every struct boundary, plus a 4 KiB block.
    let sizes = [
        0usize, 1, 2, 3, 4, 7, 8, 15, 16, 17, 18, 23, 24, 31, 32, 40, 56, 92, 108, 256, 1024, 4096,
    ];
    for &n in &sizes {
        // All zeros, all 0xFF, and several random fills.
        hammer_parsers(&vec![0u8; n]);
        hammer_parsers(&vec![0xFFu8; n]);
        for _ in 0..16 {
            let mut buf = vec![0u8; n];
            rng.fill(&mut buf);
            hammer_parsers(&buf);
        }
    }
}

#[test]
fn btree_engine_never_panics_on_arbitrary_blocks() {
    let mut rng = Lcg(0xdead_beef_cafe_babe);
    // A device full of random "blocks": node parsing must error, never panic.
    let mut data = vec![0u8; 4096 * 4];
    for _ in 0..64 {
        rng.fill(&mut data);
        let dev = MemDevice::new(data.clone());
        let bt = BtreeReader::new(&dev, 4096);
        let _ = bt.omap_get(0, 1, u64::MAX);
        let _ = bt.fs_collect(None, 0, 1, u64::MAX);
        let _ = bt.fs_walk(None, 0, u64::MAX, &mut |_, _| Ok(true));
    }
}
