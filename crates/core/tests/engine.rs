//! Integration tests for the B-tree engine and filesystem record parsing,
//! driven through synthetic in-memory nodes (rewrite plan Phase 25).

mod common;

use apfsrelic_core::apfs::btree::{split_obj_id_and_type, BtreeReader};
use apfsrelic_core::apfs::jrec::{self, DirRec, Inode};
use common::{build_fs_root_leaf, build_omap_root_leaf, obj_id_and_type, MemDevice};

#[test]
fn omap_get_picks_correct_version() {
    // Entries sorted by (oid, xid): (5,1)->0x100, (5,3)->0x200, (9,2)->0x300.
    let block = build_omap_root_leaf(&[(5, 1, 0x100), (5, 3, 0x200), (9, 2, 0x300)]);
    let dev = MemDevice::new(block);
    let bt = BtreeReader::new(&dev, common::BS as u32);

    // oid 5 at max_xid 2 -> greatest xid <= 2 is xid 1 -> 0x100.
    let e = bt.omap_get(0, 5, 2).unwrap().unwrap();
    assert_eq!(e.val.paddr, 0x100);
    assert_eq!(e.key.xid, 1);

    // oid 5 at max_xid 10 -> xid 3 -> 0x200.
    let e = bt.omap_get(0, 5, 10).unwrap().unwrap();
    assert_eq!(e.val.paddr, 0x200);

    // oid 9 -> 0x300.
    assert_eq!(bt.omap_get(0, 9, 10).unwrap().unwrap().val.paddr, 0x300);

    // Missing oid -> None (not an error).
    assert!(bt.omap_get(0, 7, 10).unwrap().is_none());
}

#[test]
fn fs_collect_and_parse_directory_records() {
    // Object 2 (root dir): an inode record + two directory entries.
    let inode_key = obj_id_and_type(2, jrec::APFS_TYPE_INODE)
        .to_le_bytes()
        .to_vec();
    let mut inode_val = vec![0u8; 92];
    // mode @80 = directory (0o040755).
    inode_val[80..82].copy_from_slice(&0o040755u16.to_le_bytes());
    // nchildren @56 = 2.
    inode_val[56..60].copy_from_slice(&2i32.to_le_bytes());

    let drec = |name: &str, file_id: u64, dt: u16| -> (Vec<u8>, Vec<u8>) {
        let mut key = obj_id_and_type(2, jrec::APFS_TYPE_DIR_REC)
            .to_le_bytes()
            .to_vec();
        let name_bytes = {
            let mut v = name.as_bytes().to_vec();
            v.push(0); // trailing NUL counted in name_len
            v
        };
        key.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes()); // name_len_and_hash
        key.extend_from_slice(&name_bytes);
        let mut val = Vec::new();
        val.extend_from_slice(&file_id.to_le_bytes()); // file_id
        val.extend_from_slice(&0u64.to_le_bytes()); // date_added
        val.extend_from_slice(&dt.to_le_bytes()); // flags (u16)
        (key, val)
    };

    // Records sorted by (obj_id, type): inode (type 3) before dir_rec (type 9).
    let records = vec![
        (inode_key, inode_val),
        drec("alpha", 0x10, jrec::DT_DIR),
        drec("béta", 0x11, jrec::DT_REG),
    ];
    let block = build_fs_root_leaf(&records);
    let dev = MemDevice::new(block);
    let bt = BtreeReader::new(&dev, common::BS as u32);

    // Physical tree (omap_root = None): collect all records for object 2.
    let recs = bt.fs_collect(None, 0, 2, u64::MAX).unwrap();
    assert_eq!(recs.len(), 3);

    // Parse the inode.
    let inode_rec = &recs[0];
    let (_oid, ty) =
        split_obj_id_and_type(u64::from_le_bytes(inode_rec.key[..8].try_into().unwrap()));
    assert_eq!(ty, jrec::APFS_TYPE_INODE);
    let inode = Inode::parse(&inode_rec.val).unwrap();
    assert!(inode.is_dir());

    // Parse the directory entries (Unicode name round-trips).
    let mut names = Vec::new();
    for rec in &recs {
        let (_oid, ty) =
            split_obj_id_and_type(u64::from_le_bytes(rec.key[..8].try_into().unwrap()));
        if ty == jrec::APFS_TYPE_DIR_REC {
            let d = DirRec::parse(&rec.key, &rec.val).unwrap();
            let type_name = d.type_name();
            names.push((d.name, d.file_id, type_name));
        }
    }
    assert_eq!(names.len(), 2);
    assert_eq!(names[0], ("alpha".to_string(), 0x10, "dir"));
    assert_eq!(names[1], ("béta".to_string(), 0x11, "file"));
}

#[test]
fn corrupt_checksum_is_a_warning_not_a_panic() {
    let mut block = build_omap_root_leaf(&[(1, 1, 0x100)]);
    // Corrupt a body byte after stamping; the stored checksum no longer matches.
    block[100] ^= 0xFF;
    let dev = MemDevice::new(block);
    let bt = BtreeReader::new(&dev, common::BS as u32);
    // Lookup still succeeds (best-effort) and a warning is recorded.
    let _ = bt.omap_get(0, 1, 10).unwrap();
    assert!(!bt.take_warnings().is_empty());
}

#[test]
fn malformed_node_geometry_errors_cleanly() {
    // A node claiming a huge key count must error, not panic or read OOB.
    let mut block = vec![0u8; common::BS];
    block[32..34].copy_from_slice(&0x0007u16.to_le_bytes()); // root|leaf|fixed
    block[36..40].copy_from_slice(&0xffff_ffffu32.to_le_bytes()); // nkeys = huge
    block[42..44].copy_from_slice(&8u16.to_le_bytes()); // tiny TOC
    let ck = apfsrelic_core::apfs::checksum::compute(&block);
    block[0..8].copy_from_slice(&ck.to_le_bytes());
    let dev = MemDevice::new(block);
    let bt = BtreeReader::new(&dev, common::BS as u32);
    assert!(bt.omap_get(0, 1, 10).is_err());
}
