//! Extended fields (`xf_blob_t` / `x_field_t`) attached to inode and dir-record
//! values (spec, "Extended Fields"; rewrite plan Phase 11).
//!
//! Layout: a 4-byte blob header (`xf_num_exts: u16`, `xf_used_data: u16`),
//! followed by `xf_num_exts` 4-byte `x_field_t` descriptors, followed by the
//! field values concatenated. **Each value is 8-byte aligned** — the cursor into
//! the value region is rounded up to a multiple of 8 before each value, matching
//! the C prototype (which was validated against real inodes). `xf_used_data` is
//! deliberately not trusted (the spec's description of it is wrong in practice).

use super::raw;
use crate::error::{corrupt, Result};

// ---- Inode extended-field types ----
pub const INO_EXT_TYPE_SNAP_XID: u8 = 1;
pub const INO_EXT_TYPE_DOCUMENT_ID: u8 = 3;
pub const INO_EXT_TYPE_NAME: u8 = 4;
pub const INO_EXT_TYPE_FINDER_INFO: u8 = 7;
pub const INO_EXT_TYPE_DSTREAM: u8 = 8;
pub const INO_EXT_TYPE_RDEV: u8 = 14;
pub const INO_EXT_TYPE_SPARSE_BYTES: u8 = 13;

// ---- Dir-record extended-field types ----
pub const DREC_EXT_TYPE_SIBLING_ID: u8 = 1;

/// One decoded extended field: its descriptor plus a copy of its value bytes.
#[derive(Debug, Clone)]
pub struct XField {
    pub x_type: u8,
    pub x_flags: u8,
    pub data: Vec<u8>,
}

/// Parse the extended-field blob beginning at the start of `b`.
///
/// Returns an empty vec if there are no extended fields. Never panics on a
/// malformed blob: a field whose descriptor or value runs off the end stops
/// parsing rather than erroring, so a partially-damaged inode still yields the
/// fields that did parse.
pub fn parse_xfields(b: &[u8]) -> Result<Vec<XField>> {
    if b.len() < 4 {
        return Ok(Vec::new());
    }
    let num = raw::u16_at(b, 0)? as usize;
    // Sanity cap: each field needs at least a 4-byte descriptor.
    if num > b.len() / 4 {
        return Err(corrupt(format!(
            "extended-field blob claims {num} fields but is only {} bytes",
            b.len()
        )));
    }

    let descs_start = 4;
    let values_start = descs_start + num * 4;
    if values_start > b.len() {
        return Err(corrupt("extended-field descriptors run past blob"));
    }

    let mut out = Vec::with_capacity(num);
    let mut value_cursor = 0usize;
    for i in 0..num {
        let d = descs_start + i * 4;
        let x_type = raw::u8_at(b, d)?;
        let x_flags = raw::u8_at(b, d + 1)?;
        let x_size = raw::u16_at(b, d + 2)? as usize;

        // 8-byte align the value cursor.
        let rem = value_cursor % 8;
        if rem != 0 {
            value_cursor += 8 - rem;
        }
        let start = values_start + value_cursor;
        let end = match start.checked_add(x_size) {
            Some(e) => e,
            None => break,
        };
        if end > b.len() {
            // Truncated value: stop, keep what parsed.
            break;
        }
        out.push(XField {
            x_type,
            x_flags,
            data: b[start..end].to_vec(),
        });
        value_cursor += x_size;
    }
    Ok(out)
}

/// Find the first extended field of the given type.
pub fn find(fields: &[XField], x_type: u8) -> Option<&XField> {
    fields.iter().find(|f| f.x_type == x_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_two_fields_with_alignment() {
        // num=2, used_data=ignored. Field A: type=NAME size=5 ("file\0").
        // Field B: type=DSTREAM size=8 (a u64). Value A is 5 bytes, so value B
        // starts at the next 8-byte boundary (offset 8 within values).
        let mut b = Vec::new();
        b.extend_from_slice(&2u16.to_le_bytes()); // num
        b.extend_from_slice(&0u16.to_le_bytes()); // used_data (unused)
        b.extend_from_slice(&[INO_EXT_TYPE_NAME, 0, 5, 0]); // desc A
        b.extend_from_slice(&[INO_EXT_TYPE_DSTREAM, 0, 8, 0]); // desc B
        b.extend_from_slice(b"file\0"); // value A (5 bytes)
        b.extend_from_slice(&[0u8; 3]); // pad to 8
        b.extend_from_slice(&0x1122u64.to_le_bytes()); // value B

        let fields = parse_xfields(&b).unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].x_type, INO_EXT_TYPE_NAME);
        assert_eq!(fields[0].data, b"file\0");
        assert_eq!(fields[1].x_type, INO_EXT_TYPE_DSTREAM);
        assert_eq!(
            u64::from_le_bytes(fields[1].data[..8].try_into().unwrap()),
            0x1122
        );
    }

    #[test]
    fn truncated_blob_does_not_panic() {
        // num=1 descriptor that fits, but claims a 200-byte value that isn't
        // there: returns the fields that fit (here, none) without panicking.
        let b = [1u8, 0, 0, 0, INO_EXT_TYPE_NAME, 0, 200, 0];
        let fields = parse_xfields(&b).unwrap();
        assert!(fields.is_empty());
    }
}
