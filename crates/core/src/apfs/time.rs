//! APFS timestamp conversion. APFS stores times as unsigned 64-bit nanoseconds
//! since the Unix epoch (1970-01-01T00:00:00Z). We format them as ISO 8601 UTC
//! without pulling in a date library, using the civil-from-days algorithm
//! (Howard Hinnant), so JSON consumers get both the raw value and a readable
//! timestamp (rewrite plan Phase 22).

/// Convert APFS nanoseconds-since-epoch to an ISO 8601 UTC string.
/// Returns `None` for value 0 (unset) so callers can omit the field.
pub fn iso8601(nanos: u64) -> Option<String> {
    if nanos == 0 {
        return None;
    }
    let secs = (nanos / 1_000_000_000) as i64;
    let sub_ns = (nanos % 1_000_000_000) as u32;

    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let hour = secs_of_day / 3600;
    let min = (secs_of_day % 3600) / 60;
    let sec = secs_of_day % 60;

    Some(if sub_ns == 0 {
        format!("{y:04}-{m:02}-{d:02}T{hour:02}:{min:02}:{sec:02}Z")
    } else {
        format!("{y:04}-{m:02}-{d:02}T{hour:02}:{min:02}:{sec:02}.{sub_ns:09}Z")
    })
}

/// Days since 1970-01-01 -> (year, month, day). Valid for the full i64 range.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_timestamps() {
        assert_eq!(iso8601(0), None);
        // 2021-01-01T00:00:00Z = 1609459200 s.
        assert_eq!(
            iso8601(1_609_459_200 * 1_000_000_000).unwrap(),
            "2021-01-01T00:00:00Z"
        );
        // With sub-second nanos.
        assert_eq!(
            iso8601(1_609_459_200 * 1_000_000_000 + 500).unwrap(),
            "2021-01-01T00:00:00.000000500Z"
        );
    }
}
