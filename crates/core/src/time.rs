//! Time conversions without a date library: Windows FILETIME, Unix milliseconds, RFC 3339 and
//! WMI's CIM datetime strings.

/// 100 ns intervals between 1601-01-01 (FILETIME epoch) and 1970-01-01 (Unix epoch).
pub const FILETIME_UNIX_DIFF: u64 = 116_444_736_000_000_000;

pub fn filetime_to_unix_ms(ft: u64) -> i64 {
    (ft as i64 - FILETIME_UNIX_DIFF as i64) / 10_000
}

pub fn unix_ms_to_filetime(ms: i64) -> u64 {
    (ms * 10_000 + FILETIME_UNIX_DIFF as i64) as u64
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `2026-10-17T14:22:03Z`. Second precision: the raw FILETIME is the identity, this is for people.
pub fn format_rfc3339(unix_ms: i64) -> String {
    let secs = unix_ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

pub fn filetime_to_rfc3339(ft: u64) -> String {
    format_rfc3339(filetime_to_unix_ms(ft))
}

/// Parses a CIM datetime (`20261017142203.123456+180`, local time plus the UTC offset in
/// minutes) into a FILETIME. Returns `None` for anything malformed.
pub fn parse_cim_datetime(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.len() < 25 || !s.is_char_boundary(14) {
        return None;
    }
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(4, 6)?, num(6, 8)?);
    let (h, mi, se) = (num(8, 10)?, num(10, 12)?, num(12, 14)?);
    if s.as_bytes()[14] != b'.' {
        return None;
    }
    let micros = num(15, 21)?;
    let sign = match s.as_bytes()[21] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let offset_min = num(22, 25)? * sign;
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let days = days_from_civil(y, mo as u32, d as u32);
    let local_secs = days * 86_400 + h * 3600 + mi * 60 + se;
    let utc_secs = local_secs - offset_min * 60;
    let ticks = utc_secs * 10_000_000 + micros * 10;
    u64::try_from(ticks + FILETIME_UNIX_DIFF as i64).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_round_numbers() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(1_792_247_323_000), "2026-10-17T14:28:43Z");
        assert_eq!(format_rfc3339(951_782_400_000), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn filetime_roundtrip() {
        let ms = 1_792_247_323_456;
        assert_eq!(filetime_to_unix_ms(unix_ms_to_filetime(ms)), ms);
        assert_eq!(
            filetime_to_rfc3339(FILETIME_UNIX_DIFF),
            "1970-01-01T00:00:00Z"
        );
    }

    #[test]
    fn cim_datetime_applies_offset() {
        // 17:28:43 at UTC+3 is 14:28:43 UTC.
        let ft = parse_cim_datetime("20261017172843.000000+180").unwrap();
        assert_eq!(filetime_to_rfc3339(ft), "2026-10-17T14:28:43Z");
        let ft = parse_cim_datetime("20261017092843.500000-300").unwrap();
        assert_eq!(filetime_to_rfc3339(ft), "2026-10-17T14:28:43Z");
        assert_eq!(filetime_to_unix_ms(ft) % 1000, 500);
    }

    #[test]
    fn cim_datetime_rejects_garbage() {
        assert_eq!(parse_cim_datetime(""), None);
        assert_eq!(parse_cim_datetime("2026101717284"), None);
        assert_eq!(parse_cim_datetime("20261317172843.000000+180"), None);
        assert_eq!(parse_cim_datetime("20261017172843x000000+180"), None);
        assert_eq!(parse_cim_datetime("20261017172843.000000*180"), None);
    }
}
