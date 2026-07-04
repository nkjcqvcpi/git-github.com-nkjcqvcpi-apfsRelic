//! Object map `omap_phys_t` and its key/value records (spec, "Object Maps";
//! rewrite plan Phase 4).

use super::obj::{ObjPhys, OBJ_PHYSICAL, OBJ_STORAGETYPE_MASK};
use super::raw;
use crate::error::Result;

// ---- omap value flags ----
pub const OMAP_VAL_DELETED: u32 = 0x1;
pub const OMAP_VAL_SAVED: u32 = 0x2;
pub const OMAP_VAL_ENCRYPTED: u32 = 0x4;
pub const OMAP_VAL_NOHEADER: u32 = 0x8;

const OFF_TREE_TYPE: usize = 40;
const OFF_TREE_OID: usize = 48;
const OFF_SNAPSHOT_TREE_OID: usize = 56;
const OFF_MOST_RECENT_SNAP: usize = 64;

/// Parsed object-map object.
#[derive(Debug, Clone)]
pub struct OmapPhys {
    pub flags: u32,
    pub snap_count: u32,
    pub tree_type: u32,
    pub snapshot_tree_type: u32,
    /// Physical OID (block address) of the object-map B-tree.
    pub tree_oid: u64,
    pub snapshot_tree_oid: u64,
    pub most_recent_snap: u64,
}

impl OmapPhys {
    pub fn parse(b: &[u8]) -> Result<OmapPhys> {
        Ok(OmapPhys {
            flags: raw::u32_at(b, 32)?,
            snap_count: raw::u32_at(b, 36)?,
            tree_type: raw::u32_at(b, OFF_TREE_TYPE)?,
            snapshot_tree_type: raw::u32_at(b, 44)?,
            tree_oid: raw::u64_at(b, OFF_TREE_OID)?,
            snapshot_tree_oid: raw::u64_at(b, OFF_SNAPSHOT_TREE_OID)?,
            most_recent_snap: raw::u64_at(b, OFF_MOST_RECENT_SNAP)?,
        })
    }

    /// True if the omap B-tree is a physical object (the only case we resolve;
    /// the C prototype bails otherwise, and so do we).
    pub fn tree_is_physical(&self) -> bool {
        self.tree_type & OBJ_STORAGETYPE_MASK == OBJ_PHYSICAL
    }
}

/// `omap_key_t` — (oid, xid). Size 16.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OmapKey {
    pub oid: u64,
    pub xid: u64,
}

impl OmapKey {
    pub const SIZE: usize = 16;
    pub fn parse(b: &[u8]) -> Result<OmapKey> {
        Ok(OmapKey {
            oid: raw::u64_at(b, 0)?,
            xid: raw::u64_at(b, 8)?,
        })
    }
}

/// `omap_val_t` — (flags, size, paddr). Size 16.
#[derive(Debug, Clone, Copy)]
pub struct OmapVal {
    pub flags: u32,
    pub size: u32,
    pub paddr: u64,
}

impl OmapVal {
    pub const SIZE: usize = 16;
    pub fn parse(b: &[u8]) -> Result<OmapVal> {
        Ok(OmapVal {
            flags: raw::u32_at(b, 0)?,
            size: raw::u32_at(b, 4)?,
            paddr: raw::u64_at(b, 8)?,
        })
    }

    pub fn is_deleted(&self) -> bool {
        self.flags & OMAP_VAL_DELETED != 0
    }
    pub fn is_encrypted(&self) -> bool {
        self.flags & OMAP_VAL_ENCRYPTED != 0
    }
    pub fn is_noheader(&self) -> bool {
        self.flags & OMAP_VAL_NOHEADER != 0
    }
}

/// A resolved object-map entry (the key that matched plus its value).
#[derive(Debug, Clone)]
pub struct OmapEntry {
    pub key: OmapKey,
    pub val: OmapVal,
}

/// Confirm an omap object's header type is `OMAP` (best-effort; callers may warn
/// rather than hard-fail).
pub fn is_omap_object(header: &ObjPhys) -> bool {
    header.type_id() == super::obj::OBJECT_TYPE_OMAP
}
