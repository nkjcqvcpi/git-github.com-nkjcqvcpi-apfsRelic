//! `Volume` — load a volume's object map and filesystem tree, and provide record
//! access used by every filesystem command (rewrite plan Phases 9, 11).

use std::io::Write;
use std::sync::Arc;

use super::btree::{split_obj_id_and_type, BtreeReader, Record};
use super::container::Container;
use super::extract::{self, Written};
use super::feature;
use super::jrec::{self, DirRec, FileExtent, Inode, Xattr};
use super::omap::OmapPhys;
use super::volume::ApfsSuperblock;
use crate::device::BlockDevice;
use crate::error::{not_found_obj, unsupported, Result};

/// xattr name that stores a symlink's target path (see `recover`).
const SYMLINK_XATTR: &str = "com.apple.fs.symlink";

/// An opened volume (live view or a snapshot view).
pub struct Volume {
    pub dev: Arc<dyn BlockDevice>,
    pub block_size: u32,
    pub apsb: ApfsSuperblock,
    /// Physical block of the volume object-map B-tree root.
    pub omap_tree_root: u64,
    /// Physical block of the filesystem root B-tree.
    pub root_tree_root: u64,
    /// XID context for resolving virtual OIDs through the volume omap.
    pub max_xid: u64,
    pub warnings: Vec<String>,
}

impl Volume {
    /// Open the live view of volume `index` within `container`.
    pub fn open(container: &Container, index: u32) -> Result<Volume> {
        let apsb = container.volume_superblock(index)?;
        Self::open_from_superblock(
            Arc::clone(&container.dev),
            container.block_size,
            apsb.clone(),
            apsb.xid,
            apsb.root_tree_oid,
        )
    }

    /// Open a view given an already-parsed volume superblock. `max_xid` is the
    /// omap-resolution context (the volume xid for the live view, or a snapshot
    /// xid); `root_tree_oid` is the virtual OID of the filesystem root tree to
    /// resolve (the volume's own, or a snapshot's).
    pub fn open_from_superblock(
        dev: Arc<dyn BlockDevice>,
        block_size: u32,
        apsb: ApfsSuperblock,
        max_xid: u64,
        root_tree_oid: u64,
    ) -> Result<Volume> {
        let mut warnings = Vec::new();

        // Feature gate: errors only on unknown incompatible bits.
        match feature::check_volume(&apsb) {
            Ok(report) => warnings.extend(report.warnings),
            Err(e) => return Err(e),
        }

        // Volume object map (physical object -> physical B-tree root).
        let omap_blk = dev.read_block(apsb.omap_oid, block_size)?;
        let omap = OmapPhys::parse(&omap_blk)?;
        if !omap.tree_is_physical() {
            return Err(unsupported("volume omap B-tree is not a physical object"));
        }
        let omap_tree_root = omap.tree_oid;

        // Resolve the (virtual) filesystem root tree through the volume omap.
        let bt = BtreeReader::new(&*dev, block_size);
        let root_entry = bt
            .omap_get(omap_tree_root, root_tree_oid, max_xid)?
            .ok_or_else(|| {
                not_found_obj(format!(
                    "filesystem root tree (virtual OID {root_tree_oid:#x}) not in volume omap"
                ))
            })?;
        let root_tree_root = root_entry.val.paddr;
        warnings.extend(bt.take_warnings());

        Ok(Volume {
            dev,
            block_size,
            apsb,
            omap_tree_root,
            root_tree_root,
            max_xid,
            warnings,
        })
    }

    /// A B-tree reader bound to this volume's device.
    pub fn btree(&self) -> BtreeReader<'_> {
        BtreeReader::new(&*self.dev, self.block_size)
    }

    /// All records for object `oid`, in key order. The volume root tree is
    /// virtual, so child links are resolved through the volume omap.
    pub fn records(&self, bt: &BtreeReader, oid: u64) -> Result<Vec<Record>> {
        bt.fs_collect(
            Some(self.omap_tree_root),
            self.root_tree_root,
            oid,
            self.max_xid,
        )
    }

    /// Parse the inode record (if any) from a record set for one object.
    pub fn inode_from_records(records: &[Record]) -> Result<Option<Inode>> {
        for rec in records {
            let (_oid, ty) = split_obj_id_and_type(crate::apfs::raw::u64_at(&rec.key, 0)?);
            if ty == jrec::APFS_TYPE_INODE {
                return Ok(Some(Inode::parse(&rec.val)?));
            }
        }
        Ok(None)
    }

    /// Look up an object's inode directly.
    pub fn inode(&self, bt: &BtreeReader, oid: u64) -> Result<Option<Inode>> {
        let records = self.records(bt, oid)?;
        Self::inode_from_records(&records)
    }

    /// List a directory's entries (the `DIR_REC` records of `dir_oid`).
    pub fn list_dir(&self, bt: &BtreeReader, dir_oid: u64) -> Result<Vec<DirRec>> {
        let records = self.records(bt, dir_oid)?;
        let mut out = Vec::new();
        for rec in &records {
            let (_oid, ty) = split_obj_id_and_type(crate::apfs::raw::u64_at(&rec.key, 0)?);
            if ty == jrec::APFS_TYPE_DIR_REC {
                out.push(DirRec::parse(&rec.key, &rec.val)?);
            }
        }
        Ok(out)
    }

    /// Reconstruct a regular file's logical data from its `FILE_EXTENT` records,
    /// writing exactly `file_size` bytes to `writer` (extents sorted by logical
    /// address, sparse holes and gaps zero-filled). Shared by the CLI `recover`
    /// command and the GUI recover action so there is a single implementation.
    pub fn write_file_data(
        &self,
        records: &[Record],
        file_size: u64,
        writer: &mut dyn Write,
    ) -> Result<Written> {
        let mut extents: Vec<FileExtent> = Vec::new();
        for rec in records {
            let (_oid, ty) = split_obj_id_and_type(crate::apfs::raw::u64_at(&rec.key, 0)?);
            if ty == jrec::APFS_TYPE_FILE_EXTENT {
                extents.push(FileExtent::parse(&rec.key, &rec.val)?);
            }
        }
        extract::write_extents(&*self.dev, self.block_size, &mut extents, file_size, writer)
    }

    /// Extract a symlink's target from its `com.apple.fs.symlink` xattr, if the
    /// records belong to a symlink and the target is stored inline.
    pub fn symlink_target(records: &[Record]) -> Result<Option<String>> {
        for rec in records {
            let (_oid, ty) = split_obj_id_and_type(crate::apfs::raw::u64_at(&rec.key, 0)?);
            if ty == jrec::APFS_TYPE_XATTR {
                let x = Xattr::parse(&rec.key, &rec.val)?;
                if x.name == SYMLINK_XATTR && x.is_embedded() {
                    return Ok(Some(crate::apfs::raw::cstr_utf8(&x.data)));
                }
            }
        }
        Ok(None)
    }
}
