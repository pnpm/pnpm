use std::time::{SystemTime, UNIX_EPOCH};

/// Generate the current time as an ISO-8601 string, matching
/// the shape npm and verdaccio use in `time.modified`.
#[must_use]
pub fn now_iso() -> String {
    let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    iso_from_unix_millis(since_epoch.as_millis() as i64)
}

/// Format a unix-millis timestamp as an ISO-8601 / RFC-3339 string
/// with millisecond precision. The token-listing endpoint surfaces
/// `TokenRecord::created_at` (seconds since epoch) through this same
/// helper so both `time.modified` on packuments and `created` on
/// tokens render with identical shape.
#[must_use]
pub fn iso_from_unix_millis(millis: i64) -> String {
    // Civil-time conversion without pulling in `chrono`.
    // 86_400_000 ms in a day.
    let (days, ms_in_day) = (millis.div_euclid(86_400_000), millis.rem_euclid(86_400_000));
    let (h, rem) = (ms_in_day / 3_600_000, ms_in_day.rem_euclid(3_600_000));
    let (m, rem) = (rem / 60_000, rem.rem_euclid(60_000));
    let (s, ms) = (rem / 1000, rem.rem_euclid(1000));
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{ms:03}Z")
}

/// Days since 1970-01-01 → year/month/day. Howard Hinnant's
/// algorithm, adapted to integer days from epoch. Shift so era
/// is positive (`719_468` = days from 0000-03-01 to 1970-01-01).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let epoch_days = days + 719_468;
    let era = epoch_days.div_euclid(146_097);
    let doe = epoch_days.rem_euclid(146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = i64::from(yoe) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}
