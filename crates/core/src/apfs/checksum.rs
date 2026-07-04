//! APFS Fletcher-64 checksum (rewrite plan Phase 5).
//!
//! Every object with an `obj_phys_t` header stores an 8-byte Fletcher-64
//! checksum in its first 8 bytes. APFS uses a variant where validating is
//! equivalent to: compute the checksum over the block treating the first two
//! 32-bit words as zero, and compare against the stored 8 bytes. This mirrors
//! the C prototype's `fletcher_cksum`, which was verified against real images.

/// Compute the stored-checksum value for `block` (first 8 bytes treated as 0).
///
/// `block` must be a whole APFS block; its length should be a multiple of 4.
pub fn compute(block: &[u8]) -> u64 {
    let modulus: u64 = 0xFFFF_FFFF;
    let num_words = block.len() / 4;
    let mut simple: u64 = 0;
    let mut second: u64 = 0;

    // Skip the first two words (the stored checksum) when computing.
    for i in 2..num_words {
        let w = u32::from_le_bytes([
            block[i * 4],
            block[i * 4 + 1],
            block[i * 4 + 2],
            block[i * 4 + 3],
        ]) as u64;
        simple = (simple + w) % modulus;
        second = (second + simple) % modulus;
    }

    let c1 = modulus - ((simple + second) % modulus);
    (second << 32) | c1
}

/// The 8-byte checksum stored at the start of a block.
pub fn stored(block: &[u8]) -> u64 {
    if block.len() < 8 {
        return 0;
    }
    let mut a = [0u8; 8];
    a.copy_from_slice(&block[..8]);
    u64::from_le_bytes(a)
}

/// True if `block`'s stored checksum matches the computed one.
pub fn is_valid(block: &[u8]) -> bool {
    if block.len() < 8 {
        return false;
    }
    compute(block) == stored(block)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_makes_block_valid() {
        // Build a 4096-byte block with arbitrary body, then stamp the checksum.
        let mut block = vec![0u8; 4096];
        for (i, byte) in block.iter_mut().enumerate().skip(8) {
            *byte = (i * 7 + 3) as u8;
        }
        let ck = compute(&block);
        block[..8].copy_from_slice(&ck.to_le_bytes());
        assert!(is_valid(&block));

        // Corrupting any body byte breaks validation.
        block[100] ^= 0xFF;
        assert!(!is_valid(&block));
    }

    #[test]
    fn short_block_is_invalid_not_panic() {
        assert!(!is_valid(&[0u8; 4]));
    }
}
