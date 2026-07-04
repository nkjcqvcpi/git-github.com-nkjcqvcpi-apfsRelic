//! Container superblock `nx_superblock_t` and checkpoint maps (spec, "Container";
//! rewrite plan Phase 4). Field offsets follow the 2020-06-22 reference exactly.

use super::raw;
use crate::error::{corrupt, Result};

/// `nx_magic` value: the four bytes "NXSB" read as a little-endian `u32`
/// (`'BSXN'` in the spec's char-constant notation).
pub const NX_MAGIC: u32 = 0x4253_584E;

pub const NX_MAX_FILE_SYSTEMS: usize = 100;

// ---- Incompatible container feature flags ----
pub const NX_INCOMPAT_VERSION1: u64 = 0x1;
pub const NX_INCOMPAT_VERSION2: u64 = 0x2;
pub const NX_INCOMPAT_FUSION: u64 = 0x100;
/// Mask of incompatible features this reader understands.
pub const NX_SUPPORTED_INCOMPAT_MASK: u64 =
    NX_INCOMPAT_VERSION1 | NX_INCOMPAT_VERSION2 | NX_INCOMPAT_FUSION;

// ---- Container flags ----
pub const NX_CRYPTO_SW: u64 = 0x4;

// Field offsets within the block (obj_phys_t header is 32 bytes).
const OFF_MAGIC: usize = 32;
const OFF_BLOCK_SIZE: usize = 36;
const OFF_BLOCK_COUNT: usize = 40;
const OFF_FEATURES: usize = 48;
const OFF_ROCOMPAT: usize = 56;
const OFF_INCOMPAT: usize = 64;
const OFF_UUID: usize = 72;
const OFF_NEXT_XID: usize = 96;
const OFF_XP_DESC_BLOCKS: usize = 104;
const OFF_XP_DATA_BLOCKS: usize = 108;
const OFF_XP_DESC_BASE: usize = 112;
const OFF_XP_DATA_BASE: usize = 120;
const OFF_XP_DESC_NEXT: usize = 128;
const OFF_XP_DATA_NEXT: usize = 132;
const OFF_XP_DESC_INDEX: usize = 136;
const OFF_XP_DESC_LEN: usize = 140;
const OFF_XP_DATA_INDEX: usize = 144;
const OFF_XP_DATA_LEN: usize = 148;
const OFF_OMAP_OID: usize = 160;
const OFF_MAX_FILE_SYSTEMS: usize = 180;
const OFF_FS_OID: usize = 184;
const OFF_FLAGS: usize = 1264;
const OFF_EFI_JUMPSTART: usize = 1272;
const OFF_KEYLOCKER: usize = 1296;

/// Parsed container superblock (only the fields this reader uses).
#[derive(Debug, Clone)]
pub struct NxSuperblock {
    pub oid: u64,
    pub xid: u64,
    pub magic: u32,
    pub block_size: u32,
    pub block_count: u64,
    pub features: u64,
    pub readonly_compatible_features: u64,
    pub incompatible_features: u64,
    pub flags: u64,
    pub uuid: String,
    pub next_xid: u64,
    /// Number of blocks in the checkpoint descriptor area. The top bit, if set,
    /// means the area is a tree rather than contiguous; [`xp_desc_is_tree`]
    /// reports that and [`xp_desc_blocks`] returns the masked block count.
    pub xp_desc_blocks_raw: u32,
    pub xp_data_blocks_raw: u32,
    pub xp_desc_base: i64,
    pub xp_data_base: i64,
    pub xp_desc_next: u32,
    pub xp_data_next: u32,
    pub xp_desc_index: u32,
    pub xp_desc_len: u32,
    pub xp_data_index: u32,
    pub xp_data_len: u32,
    pub omap_oid: u64,
    pub max_file_systems: u32,
    /// Virtual OIDs of the volume superblocks; 0 entries are absent volumes.
    pub fs_oids: Vec<u64>,
    pub efi_jumpstart: i64,
    pub keylocker_start: i64,
    pub keylocker_blocks: u64,
}

impl NxSuperblock {
    /// Parse a container superblock from a whole block.
    pub fn parse(b: &[u8]) -> Result<NxSuperblock> {
        let header = super::obj::ObjPhys::parse(b)?;
        let max_fs = raw::u32_at(b, OFF_MAX_FILE_SYSTEMS)?;
        // Clamp the number of fs_oid entries we read to the spec maximum so a
        // corrupt `max_file_systems` can't make us read past the array.
        let count = (max_fs as usize).min(NX_MAX_FILE_SYSTEMS);
        let mut fs_oids = Vec::with_capacity(count);
        for i in 0..count {
            fs_oids.push(raw::u64_at(b, OFF_FS_OID + i * 8)?);
        }

        Ok(NxSuperblock {
            oid: header.oid,
            xid: header.xid,
            magic: raw::u32_at(b, OFF_MAGIC)?,
            block_size: raw::u32_at(b, OFF_BLOCK_SIZE)?,
            block_count: raw::u64_at(b, OFF_BLOCK_COUNT)?,
            features: raw::u64_at(b, OFF_FEATURES)?,
            readonly_compatible_features: raw::u64_at(b, OFF_ROCOMPAT)?,
            incompatible_features: raw::u64_at(b, OFF_INCOMPAT)?,
            flags: raw::u64_at(b, OFF_FLAGS)?,
            uuid: raw::uuid_at(b, OFF_UUID)?,
            next_xid: raw::u64_at(b, OFF_NEXT_XID)?,
            xp_desc_blocks_raw: raw::u32_at(b, OFF_XP_DESC_BLOCKS)?,
            xp_data_blocks_raw: raw::u32_at(b, OFF_XP_DATA_BLOCKS)?,
            xp_desc_base: raw::i64_at(b, OFF_XP_DESC_BASE)?,
            xp_data_base: raw::i64_at(b, OFF_XP_DATA_BASE)?,
            xp_desc_next: raw::u32_at(b, OFF_XP_DESC_NEXT)?,
            xp_data_next: raw::u32_at(b, OFF_XP_DATA_NEXT)?,
            xp_desc_index: raw::u32_at(b, OFF_XP_DESC_INDEX)?,
            xp_desc_len: raw::u32_at(b, OFF_XP_DESC_LEN)?,
            xp_data_index: raw::u32_at(b, OFF_XP_DATA_INDEX)?,
            xp_data_len: raw::u32_at(b, OFF_XP_DATA_LEN)?,
            omap_oid: raw::u64_at(b, OFF_OMAP_OID)?,
            max_file_systems: max_fs,
            fs_oids,
            efi_jumpstart: raw::i64_at(b, OFF_EFI_JUMPSTART)?,
            keylocker_start: raw::i64_at(b, OFF_KEYLOCKER)?,
            keylocker_blocks: raw::u64_at(b, OFF_KEYLOCKER + 8)?,
        })
    }

    /// True if the checkpoint descriptor area is stored as a tree (top bit set).
    pub fn xp_desc_is_tree(&self) -> bool {
        self.xp_desc_blocks_raw & (1 << 31) != 0
    }

    /// Number of blocks in the (contiguous) checkpoint descriptor area.
    pub fn xp_desc_blocks(&self) -> u32 {
        self.xp_desc_blocks_raw & !(1 << 31)
    }

    /// Validate the magic number.
    pub fn check_magic(&self) -> Result<()> {
        if self.magic != NX_MAGIC {
            return Err(corrupt(format!(
                "container superblock has bad magic {:#x} (expected NXSB)",
                self.magic
            )));
        }
        Ok(())
    }

    /// A keybag/software-crypto presence hint at the container level.
    pub fn has_keylocker(&self) -> bool {
        self.keylocker_start != 0 && self.keylocker_blocks != 0
    }
}

/// One checkpoint mapping entry (`checkpoint_mapping_t`, 40 bytes).
#[derive(Debug, Clone)]
pub struct CheckpointMapping {
    pub cpm_type: u32,
    pub cpm_subtype: u32,
    pub cpm_size: u32,
    pub fs_oid: u64,
    pub oid: u64,
    pub paddr: u64,
}

const CPM_ENTRY_SIZE: usize = 40;
const CPM_FLAGS_OFF: usize = 32;
const CPM_COUNT_OFF: usize = 36;
const CPM_MAP_OFF: usize = 40;

/// A checkpoint map block (`checkpoint_map_phys_t`).
#[derive(Debug, Clone)]
pub struct CheckpointMap {
    pub flags: u32,
    pub mappings: Vec<CheckpointMapping>,
}

/// `CHECKPOINT_MAP_LAST` flag: this is the last map block of the checkpoint.
pub const CHECKPOINT_MAP_LAST: u32 = 0x1;

impl CheckpointMap {
    /// Parse a checkpoint map block.
    pub fn parse(b: &[u8]) -> Result<CheckpointMap> {
        let flags = raw::u32_at(b, CPM_FLAGS_OFF)?;
        let count = raw::u32_at(b, CPM_COUNT_OFF)? as usize;
        // The map array must fit within the block.
        let max_by_block = b.len().saturating_sub(CPM_MAP_OFF) / CPM_ENTRY_SIZE;
        if count > max_by_block {
            return Err(corrupt(format!(
                "checkpoint map count {count} exceeds {max_by_block} that fit in the block"
            )));
        }
        let mut mappings = Vec::with_capacity(count);
        for i in 0..count {
            let off = CPM_MAP_OFF + i * CPM_ENTRY_SIZE;
            mappings.push(CheckpointMapping {
                cpm_type: raw::u32_at(b, off)?,
                cpm_subtype: raw::u32_at(b, off + 4)?,
                cpm_size: raw::u32_at(b, off + 8)?,
                fs_oid: raw::u64_at(b, off + 16)?,
                oid: raw::u64_at(b, off + 24)?,
                paddr: raw::u64_at(b, off + 32)?,
            });
        }
        Ok(CheckpointMap { flags, mappings })
    }

    pub fn is_last(&self) -> bool {
        self.flags & CHECKPOINT_MAP_LAST != 0
    }
}
