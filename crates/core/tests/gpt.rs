//! Integration test for GPT partition discovery and container-window selection
//! (rewrite plan Phase 3).

mod common;

use apfsrelic_core::device::partition::{
    resolve_container_window, PartitionSelector, PartitionTable,
};
use common::MemDevice;

const LBA: usize = 512;

/// Apple_APFS type GUID in on-disk byte order.
const APFS_GUID: [u8; 16] = [
    0xEF, 0x57, 0x34, 0x7C, 0x00, 0x00, 0xAA, 0x11, 0xAA, 0x11, 0x00, 0x30, 0x65, 0x43, 0xEC, 0xAC,
];
/// EFI System type GUID (arbitrary non-APFS).
const EFI_GUID: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];

fn put_u32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
fn put_u64(b: &mut [u8], off: usize, v: u64) {
    b[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

fn build_gpt_image() -> Vec<u8> {
    // 64 sectors: LBA0 MBR, LBA1 GPT header, LBA2 entries, then payload.
    let mut img = vec![0u8; 64 * LBA];

    // GPT header at LBA1.
    let h = LBA;
    img[h..h + 8].copy_from_slice(b"EFI PART");
    put_u64(&mut img, h + 72, 2); // partition_entry_lba
    put_u32(&mut img, h + 80, 4); // num_entries
    put_u32(&mut img, h + 84, 128); // entry_size

    // Entry 0 at LBA2 = EFI (first_lba 6, last 9).
    let e0 = 2 * LBA;
    img[e0..e0 + 16].copy_from_slice(&EFI_GUID);
    img[e0 + 16..e0 + 32].copy_from_slice(&[0x11; 16]); // unique guid
    put_u64(&mut img, e0 + 32, 6);
    put_u64(&mut img, e0 + 40, 9);

    // Entry 1 = APFS (first_lba 10, last 40).
    let e1 = e0 + 128;
    img[e1..e1 + 16].copy_from_slice(&APFS_GUID);
    img[e1 + 16..e1 + 32].copy_from_slice(&[0x22; 16]);
    put_u64(&mut img, e1 + 32, 10);
    put_u64(&mut img, e1 + 40, 40);

    img
}

#[test]
fn parses_gpt_and_finds_apfs_partition() {
    let dev = MemDevice::new(build_gpt_image());
    let table = PartitionTable::parse(&dev).unwrap().expect("has GPT");
    assert_eq!(table.entries.len(), 2);

    let apfs = table.apfs_partitions();
    assert_eq!(apfs.len(), 1);
    assert_eq!(apfs[0].type_guid, "7C3457EF-0000-11AA-AA11-00306543ECAC");
    assert_eq!(apfs[0].first_lba, 10);
    assert_eq!(apfs[0].byte_offset(), 10 * 512);

    // Auto selection lands on the APFS partition.
    let (off, len, idx) = resolve_container_window(&dev, PartitionSelector::Auto, None).unwrap();
    assert_eq!(off, 10 * 512);
    assert_eq!(len, (40 - 10 + 1) * 512);
    assert_eq!(idx, Some(2));
}

#[test]
fn explicit_offset_overrides_gpt() {
    let dev = MemDevice::new(build_gpt_image());
    let (off, _len, idx) =
        resolve_container_window(&dev, PartitionSelector::Auto, Some(2048)).unwrap();
    assert_eq!(off, 2048);
    assert_eq!(idx, None);
}

#[test]
fn no_gpt_means_bare_container() {
    // No "EFI PART" magic -> treated as a bare container at offset 0.
    let dev = MemDevice::new(vec![0u8; 8192]);
    assert!(PartitionTable::parse(&dev).unwrap().is_none());
    let (off, len, idx) = resolve_container_window(&dev, PartitionSelector::Auto, None).unwrap();
    assert_eq!(off, 0);
    assert_eq!(len, 8192);
    assert_eq!(idx, None);
}
