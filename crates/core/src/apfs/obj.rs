//! The common object header `obj_phys_t` and object type/flag constants
//! (spec, "Objects"; rewrite plan Phase 4).

use super::raw;
use crate::error::Result;

/// Size of `obj_phys_t` on disk.
pub const OBJ_PHYS_SIZE: usize = 32;

// ---- Object type masks ----
pub const OBJECT_TYPE_MASK: u32 = 0x0000_ffff;
pub const OBJECT_TYPE_FLAGS_MASK: u32 = 0xffff_0000;
pub const OBJ_STORAGETYPE_MASK: u32 = 0xc000_0000;

// ---- Object types (low 16 bits of o_type) ----
pub const OBJECT_TYPE_NX_SUPERBLOCK: u32 = 0x01;
pub const OBJECT_TYPE_BTREE: u32 = 0x02;
pub const OBJECT_TYPE_BTREE_NODE: u32 = 0x03;
pub const OBJECT_TYPE_OMAP: u32 = 0x0b;
pub const OBJECT_TYPE_CHECKPOINT_MAP: u32 = 0x0c;
pub const OBJECT_TYPE_FS: u32 = 0x0d;
pub const OBJECT_TYPE_FSTREE: u32 = 0x0e;
pub const OBJECT_TYPE_BLOCKREFTREE: u32 = 0x0f;
pub const OBJECT_TYPE_SNAPMETATREE: u32 = 0x10;
pub const OBJECT_TYPE_OMAP_SNAPSHOT: u32 = 0x13;
pub const OBJECT_TYPE_SNAP_META_EXT: u32 = 0x1d;
pub const OBJECT_TYPE_INTEGRITY_META: u32 = 0x1e;
pub const OBJECT_TYPE_FEXT_TREE: u32 = 0x1f;

// ---- Object type flags (high 16 bits of o_type) ----
pub const OBJ_VIRTUAL: u32 = 0x0000_0000;
pub const OBJ_EPHEMERAL: u32 = 0x8000_0000;
pub const OBJ_PHYSICAL: u32 = 0x4000_0000;
pub const OBJ_NOHEADER: u32 = 0x2000_0000;
pub const OBJ_ENCRYPTED: u32 = 0x1000_0000;
pub const OBJ_NONPERSISTENT: u32 = 0x0800_0000;

/// Storage class of an object, derived from `o_type`'s storage bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageType {
    /// Resolved through an object map (`OBJ_VIRTUAL`).
    Virtual,
    /// Addressed directly by block number (`OBJ_PHYSICAL`).
    Physical,
    /// Resolved through a checkpoint map (`OBJ_EPHEMERAL`).
    Ephemeral,
}

/// The parsed common object header present at the start of most APFS objects.
#[derive(Debug, Clone)]
pub struct ObjPhys {
    /// Stored Fletcher-64 checksum (raw 8 bytes as `u64`).
    pub cksum: u64,
    /// Object identifier.
    pub oid: u64,
    /// Transaction identifier.
    pub xid: u64,
    /// Raw `o_type` (type + flags + storage bits).
    pub otype: u32,
    /// Raw `o_subtype`.
    pub subtype: u32,
}

impl ObjPhys {
    /// Parse the 32-byte header at the start of `b`.
    pub fn parse(b: &[u8]) -> Result<ObjPhys> {
        Ok(ObjPhys {
            cksum: raw::u64_at(b, 0)?,
            oid: raw::u64_at(b, 8)?,
            xid: raw::u64_at(b, 16)?,
            otype: raw::u32_at(b, 24)?,
            subtype: raw::u32_at(b, 28)?,
        })
    }

    /// The object type proper (low 16 bits).
    pub fn type_id(&self) -> u32 {
        self.otype & OBJECT_TYPE_MASK
    }

    /// Storage class from the top two type bits.
    pub fn storage_type(&self) -> StorageType {
        match self.otype & OBJ_STORAGETYPE_MASK {
            OBJ_PHYSICAL => StorageType::Physical,
            OBJ_EPHEMERAL => StorageType::Ephemeral,
            _ => StorageType::Virtual,
        }
    }

    pub fn is_encrypted(&self) -> bool {
        self.otype & OBJ_ENCRYPTED != 0
    }

    pub fn has_no_header(&self) -> bool {
        self.otype & OBJ_NOHEADER != 0
    }
}

/// Human name for a known object type id (the low 16 bits).
pub fn type_name(type_id: u32) -> &'static str {
    match type_id {
        OBJECT_TYPE_NX_SUPERBLOCK => "nx_superblock",
        OBJECT_TYPE_BTREE => "btree",
        OBJECT_TYPE_BTREE_NODE => "btree_node",
        OBJECT_TYPE_OMAP => "omap",
        OBJECT_TYPE_CHECKPOINT_MAP => "checkpoint_map",
        OBJECT_TYPE_FS => "fs",
        OBJECT_TYPE_FSTREE => "fstree",
        OBJECT_TYPE_BLOCKREFTREE => "blockreftree",
        OBJECT_TYPE_SNAPMETATREE => "snapmetatree",
        OBJECT_TYPE_OMAP_SNAPSHOT => "omap_snapshot",
        OBJECT_TYPE_SNAP_META_EXT => "snap_meta_ext",
        OBJECT_TYPE_INTEGRITY_META => "integrity_meta",
        OBJECT_TYPE_FEXT_TREE => "fext_tree",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_header_and_storage_type() {
        let mut b = vec![0u8; 32];
        b[8..16].copy_from_slice(&1u64.to_le_bytes()); // oid
        b[16..24].copy_from_slice(&7u64.to_le_bytes()); // xid
        b[24..28].copy_from_slice(&(OBJ_EPHEMERAL | OBJECT_TYPE_NX_SUPERBLOCK).to_le_bytes());
        let o = ObjPhys::parse(&b).unwrap();
        assert_eq!(o.oid, 1);
        assert_eq!(o.xid, 7);
        assert_eq!(o.type_id(), OBJECT_TYPE_NX_SUPERBLOCK);
        assert_eq!(o.storage_type(), StorageType::Ephemeral);
    }

    #[test]
    fn short_header_errors() {
        assert!(ObjPhys::parse(&[0u8; 16]).is_err());
    }
}
