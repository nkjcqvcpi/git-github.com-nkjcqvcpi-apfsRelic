#![no_main]
//! Fuzz the on-disk struct and filesystem-record parsers. None may panic.

use libfuzzer_sys::fuzz_target;

use apfsrelic_core::apfs::jrec::{DirRec, FileExtent, Inode, SnapMetadata, SnapName, Xattr};
use apfsrelic_core::apfs::nx::{CheckpointMap, NxSuperblock};
use apfsrelic_core::apfs::obj::ObjPhys;
use apfsrelic_core::apfs::omap::{OmapKey, OmapPhys, OmapVal};
use apfsrelic_core::apfs::volume::ApfsSuperblock;
use apfsrelic_core::apfs::xfield::parse_xfields;

fuzz_target!(|data: &[u8]| {
    let _ = ObjPhys::parse(data);
    let _ = NxSuperblock::parse(data);
    let _ = CheckpointMap::parse(data);
    let _ = OmapPhys::parse(data);
    let _ = OmapKey::parse(data);
    let _ = OmapVal::parse(data);
    let _ = ApfsSuperblock::parse(data);
    let _ = parse_xfields(data);
    let _ = Inode::parse(data);
    let _ = SnapMetadata::parse(data);

    let mid = data.len() / 2;
    let (k, v) = data.split_at(mid);
    let _ = DirRec::parse(k, v);
    let _ = FileExtent::parse(k, v);
    let _ = Xattr::parse(k, v);
    let _ = SnapName::parse(k, v);
});
