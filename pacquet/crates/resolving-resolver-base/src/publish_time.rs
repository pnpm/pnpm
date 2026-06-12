//! Lenient parsing of npm registry publish timestamps.

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};

/// Parse a publish timestamp from registry metadata into a UTC
/// instant, or `None` when the string is not a recognized form.
///
/// npm stamps full RFC 3339 (`2024-03-15T09:42:13.123Z`), but pnpr
/// coarsens those timestamps to shrink the abbreviated packument it
/// serves: seconds come off every entry (`2024-03-15T09:42Z`), and
/// entries older than a week lose the time-of-day entirely
/// (`2024-03-15`, read as midnight UTC). pnpm's JavaScript consumers
/// accept all three through `new Date(...)`; this is the matching
/// tolerance for pacquet, whose [`DateTime::parse_from_rfc3339`] alone
/// rejects the two shorter forms.
#[must_use]
pub fn parse_packument_timestamp(input: &str) -> Option<DateTime<Utc>> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(input) {
        return Some(parsed.with_timezone(&Utc));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(input, "%Y-%m-%dT%H:%MZ") {
        return Some(Utc.from_utc_datetime(&naive));
    }
    if let Ok(date) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        return Some(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?));
    }
    None
}

#[cfg(test)]
mod tests;
