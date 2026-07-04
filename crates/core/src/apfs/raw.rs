//! Bounds-checked, little-endian primitive reads over a byte slice.
//!
//! Every multi-byte integer in an APFS structure is little-endian (spec, "Basic
//! Types"). These helpers return `Result` on out-of-range access so no parser
//! ever indexes out of bounds or panics on a short buffer (AGENTS.md rule 2).

use crate::error::{corrupt, Result};

/// Read a `u8` at `off`.
pub fn u8_at(b: &[u8], off: usize) -> Result<u8> {
    b.get(off).copied().ok_or_else(|| {
        corrupt(format!(
            "u8 read past end of {}-byte buffer at {off}",
            b.len()
        ))
    })
}

/// Read a little-endian `u16` at `off`.
pub fn u16_at(b: &[u8], off: usize) -> Result<u16> {
    let s = slice(b, off, 2)?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

/// Read a little-endian `u32` at `off`.
pub fn u32_at(b: &[u8], off: usize) -> Result<u32> {
    let s = slice(b, off, 4)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Read a little-endian `u64` at `off`.
pub fn u64_at(b: &[u8], off: usize) -> Result<u64> {
    let s = slice(b, off, 8)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Ok(u64::from_le_bytes(a))
}

/// Read a little-endian `i64` at `off` (used for `paddr_t`).
pub fn i64_at(b: &[u8], off: usize) -> Result<i64> {
    Ok(u64_at(b, off)? as i64)
}

/// Read a little-endian `i32` at `off`.
pub fn i32_at(b: &[u8], off: usize) -> Result<i32> {
    Ok(u32_at(b, off)? as i32)
}

/// Borrow `len` bytes at `off`, bounds-checked.
pub fn slice(b: &[u8], off: usize, len: usize) -> Result<&[u8]> {
    let end = off
        .checked_add(len)
        .ok_or_else(|| corrupt("length overflow in slice"))?;
    b.get(off..end).ok_or_else(|| {
        corrupt(format!(
            "slice [{off}, {end}) out of {}-byte buffer",
            b.len()
        ))
    })
}

/// Read a 16-byte UUID at `off` and format it canonically.
pub fn uuid_at(b: &[u8], off: usize) -> Result<String> {
    let s = slice(b, off, 16)?;
    Ok(format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
        s[8], s[9], s[10], s[11], s[12], s[13], s[14], s[15]
    ))
}

/// Decode a NUL-terminated UTF-8 string from a fixed field. The APFS volume
/// name and file names are UTF-8; invalid bytes are replaced rather than
/// rejected so a slightly damaged name still renders.
pub fn cstr_utf8(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_reads() {
        let b = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        assert_eq!(u16_at(&b, 0).unwrap(), 0x0201);
        assert_eq!(u32_at(&b, 0).unwrap(), 0x04030201);
        assert_eq!(u64_at(&b, 0).unwrap(), 0x0807060504030201);
    }

    #[test]
    fn short_buffer_errors_not_panics() {
        let b = [0u8; 4];
        assert!(u32_at(&b, 0).is_ok());
        assert!(u64_at(&b, 0).is_err());
        assert!(u16_at(&b, 3).is_err());
        assert!(slice(&b, 2, 4).is_err());
    }

    #[test]
    fn cstr_stops_at_nul() {
        assert_eq!(cstr_utf8(b"hello\0world"), "hello");
        assert_eq!(cstr_utf8(b"nonul"), "nonul");
    }
}
