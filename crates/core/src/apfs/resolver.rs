//! Object resolver status types (rewrite plan Phase 7).
//!
//! The actual traversal lives in [`crate::apfs::btree::BtreeReader`]; this module
//! adds the richer *status* vocabulary the rewrite plan calls for, so callers can
//! distinguish "deleted" from "missing" from "encrypted" without duplicating
//! object-map logic. `ls`, `stat`, `recover`, and `inspect` all resolve through
//! the same `BtreeReader`, then classify with [`classify`].

use super::btree::BtreeReader;
use super::omap::OmapEntry;
use crate::error::Result;

/// The outcome of resolving a virtual object through an object map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveStatus {
    /// Found at the given physical block address.
    Found { paddr: u64, xid: u64 },
    /// No entry for this OID at or below the requested XID.
    NotFound,
    /// The entry is present but flagged deleted.
    Deleted,
    /// The entry is flagged encrypted (object-level).
    Encrypted,
}

/// Classify an optional omap entry into a [`ResolveStatus`].
pub fn classify(entry: Option<OmapEntry>) -> ResolveStatus {
    match entry {
        None => ResolveStatus::NotFound,
        Some(e) if e.val.is_deleted() => ResolveStatus::Deleted,
        Some(e) if e.val.is_encrypted() => ResolveStatus::Encrypted,
        Some(e) => ResolveStatus::Found {
            paddr: e.val.paddr,
            xid: e.key.xid,
        },
    }
}

/// Resolve a virtual OID through a physical omap tree and classify the result.
pub fn resolve_virtual(
    bt: &BtreeReader,
    omap_root: u64,
    oid: u64,
    max_xid: u64,
) -> Result<ResolveStatus> {
    Ok(classify(bt.omap_get(omap_root, oid, max_xid)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apfs::omap::{OmapKey, OmapVal, OMAP_VAL_DELETED, OMAP_VAL_ENCRYPTED};

    fn entry(flags: u32) -> OmapEntry {
        OmapEntry {
            key: OmapKey { oid: 5, xid: 9 },
            val: OmapVal {
                flags,
                size: 4096,
                paddr: 0x1234,
            },
        }
    }

    #[test]
    fn classifies_statuses() {
        assert_eq!(classify(None), ResolveStatus::NotFound);
        assert_eq!(
            classify(Some(entry(OMAP_VAL_DELETED))),
            ResolveStatus::Deleted
        );
        assert_eq!(
            classify(Some(entry(OMAP_VAL_ENCRYPTED))),
            ResolveStatus::Encrypted
        );
        assert_eq!(
            classify(Some(entry(0))),
            ResolveStatus::Found {
                paddr: 0x1234,
                xid: 9
            }
        );
    }
}
