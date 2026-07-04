//! Volume superblock `apfs_superblock_t` (spec, "Volumes"; rewrite plan Phase 4
//! and Phase 9).
//!
//! Field offsets account for the 2 bytes of struct padding the C compiler (and
//! thus Apple's on-disk layout) inserts after the 18-byte
//! `wrapped_meta_crypto_state_t` and before `apfs_root_tree_type`.

use super::raw;
use crate::error::{corrupt, Result};

/// `apfs_magic` value: bytes "APSB" as a little-endian `u32` (`'BSPA'`).
pub const APFS_MAGIC: u32 = 0x4253_5041;

pub const APFS_VOLNAME_LEN: usize = 256;

// ---- Volume flags (apfs_fs_flags) ----
pub const APFS_FS_UNENCRYPTED: u64 = 0x1;
pub const APFS_FS_ONEKEY: u64 = 0x8;

// ---- Incompatible volume feature flags ----
pub const APFS_INCOMPAT_CASE_INSENSITIVE: u64 = 0x1;
pub const APFS_INCOMPAT_DATALESS_SNAPS: u64 = 0x2;
pub const APFS_INCOMPAT_ENC_ROLLED: u64 = 0x4;
pub const APFS_INCOMPAT_NORMALIZATION_INSENSITIVE: u64 = 0x8;
pub const APFS_INCOMPAT_INCOMPLETE_RESTORE: u64 = 0x10;
pub const APFS_INCOMPAT_SEALED_VOLUME: u64 = 0x20;
/// Mask of incompatible volume features this reader understands enough to browse.
pub const APFS_SUPPORTED_INCOMPAT_MASK: u64 = APFS_INCOMPAT_CASE_INSENSITIVE
    | APFS_INCOMPAT_DATALESS_SNAPS
    | APFS_INCOMPAT_ENC_ROLLED
    | APFS_INCOMPAT_NORMALIZATION_INSENSITIVE
    | APFS_INCOMPAT_INCOMPLETE_RESTORE
    | APFS_INCOMPAT_SEALED_VOLUME;

const OFF_MAGIC: usize = 32;
const OFF_FS_INDEX: usize = 36;
const OFF_FEATURES: usize = 40;
const OFF_ROCOMPAT: usize = 48;
const OFF_INCOMPAT: usize = 56;
const OFF_ROOT_TREE_TYPE: usize = 116;
const OFF_EXTENTREF_TREE_TYPE: usize = 120;
const OFF_SNAP_META_TREE_TYPE: usize = 124;
const OFF_OMAP_OID: usize = 128;
const OFF_ROOT_TREE_OID: usize = 136;
const OFF_EXTENTREF_TREE_OID: usize = 144;
const OFF_SNAP_META_TREE_OID: usize = 152;
const OFF_NUM_FILES: usize = 184;
const OFF_NUM_DIRECTORIES: usize = 192;
const OFF_NUM_SYMLINKS: usize = 200;
const OFF_NUM_OTHER: usize = 208;
const OFF_NUM_SNAPSHOTS: usize = 216;
const OFF_VOL_UUID: usize = 240;
const OFF_LAST_MOD_TIME: usize = 256;
const OFF_FS_FLAGS: usize = 264;
const OFF_VOLNAME: usize = 704;
const OFF_ROLE: usize = 964;
const OFF_SNAP_META_EXT_OID: usize = 1000;
const OFF_VOLUME_GROUP_ID: usize = 1008;
const OFF_INTEGRITY_META_OID: usize = 1024;
const OFF_FEXT_TREE_OID: usize = 1032;

/// Parsed volume superblock (the fields this reader uses).
#[derive(Debug, Clone)]
pub struct ApfsSuperblock {
    pub oid: u64,
    pub xid: u64,
    pub magic: u32,
    pub fs_index: u32,
    pub features: u64,
    pub readonly_compatible_features: u64,
    pub incompatible_features: u64,
    pub omap_oid: u64,
    pub root_tree_oid: u64,
    pub extentref_tree_oid: u64,
    pub snap_meta_tree_oid: u64,
    pub root_tree_type: u32,
    pub extentref_tree_type: u32,
    pub snap_meta_tree_type: u32,
    pub num_files: u64,
    pub num_directories: u64,
    pub num_symlinks: u64,
    pub num_other_fsobjects: u64,
    pub num_snapshots: u64,
    pub vol_uuid: String,
    pub last_mod_time: u64,
    pub fs_flags: u64,
    pub volname: String,
    pub role: u16,
    pub snap_meta_ext_oid: u64,
    pub volume_group_id: String,
    pub integrity_meta_oid: u64,
    pub fext_tree_oid: u64,
}

impl ApfsSuperblock {
    pub fn parse(b: &[u8]) -> Result<ApfsSuperblock> {
        let header = super::obj::ObjPhys::parse(b)?;
        let volname_bytes = raw::slice(b, OFF_VOLNAME, APFS_VOLNAME_LEN)?;
        Ok(ApfsSuperblock {
            oid: header.oid,
            xid: header.xid,
            magic: raw::u32_at(b, OFF_MAGIC)?,
            fs_index: raw::u32_at(b, OFF_FS_INDEX)?,
            features: raw::u64_at(b, OFF_FEATURES)?,
            readonly_compatible_features: raw::u64_at(b, OFF_ROCOMPAT)?,
            incompatible_features: raw::u64_at(b, OFF_INCOMPAT)?,
            omap_oid: raw::u64_at(b, OFF_OMAP_OID)?,
            root_tree_oid: raw::u64_at(b, OFF_ROOT_TREE_OID)?,
            extentref_tree_oid: raw::u64_at(b, OFF_EXTENTREF_TREE_OID)?,
            snap_meta_tree_oid: raw::u64_at(b, OFF_SNAP_META_TREE_OID)?,
            root_tree_type: raw::u32_at(b, OFF_ROOT_TREE_TYPE)?,
            extentref_tree_type: raw::u32_at(b, OFF_EXTENTREF_TREE_TYPE)?,
            snap_meta_tree_type: raw::u32_at(b, OFF_SNAP_META_TREE_TYPE)?,
            num_files: raw::u64_at(b, OFF_NUM_FILES)?,
            num_directories: raw::u64_at(b, OFF_NUM_DIRECTORIES)?,
            num_symlinks: raw::u64_at(b, OFF_NUM_SYMLINKS)?,
            num_other_fsobjects: raw::u64_at(b, OFF_NUM_OTHER)?,
            num_snapshots: raw::u64_at(b, OFF_NUM_SNAPSHOTS)?,
            vol_uuid: raw::uuid_at(b, OFF_VOL_UUID)?,
            last_mod_time: raw::u64_at(b, OFF_LAST_MOD_TIME)?,
            fs_flags: raw::u64_at(b, OFF_FS_FLAGS)?,
            volname: raw::cstr_utf8(volname_bytes),
            role: raw::u16_at(b, OFF_ROLE)?,
            snap_meta_ext_oid: raw::u64_at(b, OFF_SNAP_META_EXT_OID)?,
            volume_group_id: raw::uuid_at(b, OFF_VOLUME_GROUP_ID)?,
            integrity_meta_oid: raw::u64_at(b, OFF_INTEGRITY_META_OID)?,
            fext_tree_oid: raw::u64_at(b, OFF_FEXT_TREE_OID)?,
        })
    }

    pub fn check_magic(&self) -> Result<()> {
        if self.magic != APFS_MAGIC {
            return Err(corrupt(format!(
                "volume superblock has bad magic {:#x} (expected APSB)",
                self.magic
            )));
        }
        Ok(())
    }

    pub fn is_case_insensitive(&self) -> bool {
        self.incompatible_features & APFS_INCOMPAT_CASE_INSENSITIVE != 0
    }

    pub fn is_normalization_insensitive(&self) -> bool {
        self.incompatible_features & APFS_INCOMPAT_NORMALIZATION_INSENSITIVE != 0
    }

    pub fn is_sealed(&self) -> bool {
        self.incompatible_features & APFS_INCOMPAT_SEALED_VOLUME != 0
    }

    /// True if the volume is software-encrypted (the `UNENCRYPTED` flag is clear).
    pub fn is_encrypted(&self) -> bool {
        self.fs_flags & APFS_FS_UNENCRYPTED == 0
    }

    pub fn role_name(&self) -> &'static str {
        role_name(self.role)
    }
}

/// Human name for a volume role value.
pub fn role_name(role: u16) -> &'static str {
    match role {
        0x0000 => "none",
        0x0001 => "system",
        0x0002 => "user",
        0x0004 => "recovery",
        0x0008 => "vm",
        0x0010 => "preboot",
        0x0020 => "installer",
        0x0040 => "data",
        0x0080 => "baseband",
        0x00c0 => "update",
        0x0100 => "xart",
        0x0140 => "hardware",
        0x0180 => "backup",
        0x02c0 => "prelogin",
        _ => "unknown",
    }
}
