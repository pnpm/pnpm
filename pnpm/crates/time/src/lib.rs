//! Timestamp formats shared by pnpm and pnpr.

use jiff::{
    Timestamp,
    civil::{Date, DateTime},
    fmt::temporal::{DateTimeParser, DateTimePrinter},
};

/// Parse an RFC 3339 timestamp, requiring seconds and an explicit offset.
pub fn parse_rfc3339(input: &str) -> Result<Timestamp, &'static str> {
    Timestamp::from_nanosecond(parse_rfc3339_nanos(input)?).map_err(|_| "timestamp out of range")
}

/// Parse RFC 3339 into Unix milliseconds, including the entire year 9999.
pub fn parse_rfc3339_millis(input: &str) -> Result<i64, &'static str> {
    i64::try_from(parse_rfc3339_nanos(input)?.div_euclid(1_000_000))
        .map_err(|_| "timestamp out of range")
}

fn parse_rfc3339_nanos(input: &str) -> Result<i128, &'static str> {
    if input.len() < 20 || input.as_bytes().get(16) != Some(&b':') {
        return Err("invalid RFC 3339 timestamp");
    }
    let suffix = input
        .get(19..)
        .ok_or("invalid RFC 3339 timestamp")?;
    let offset = suffix
        .strip_prefix('.')
        .map(|fraction| fraction.trim_start_matches(|character: char| character.is_ascii_digit()))
        .unwrap_or(suffix);
    if !is_rfc3339_offset(offset) {
        return Err("invalid RFC 3339 offset");
    }
    let normalized;
    let parsed_input = if suffix.len() - offset.len() > 10 {
        normalized = format!("{}{offset}", &input[..29]);
        normalized.as_str()
    } else {
        input
    };
    let pieces = DateTimeParser::new()
        .parse_pieces(parsed_input)
        .map_err(|_| "invalid RFC 3339 timestamp")?;
    let civil = pieces
        .date()
        .to_datetime(pieces.time().ok_or("missing time")?);
    let offset = pieces.to_numeric_offset().ok_or("missing offset")?;
    let nanos = civil
        .duration_since(jiff::civil::date(1970, 1, 1).at(0, 0, 0, 0))
        .as_nanos()
        - i128::from(offset.seconds()) * 1_000_000_000;
    // Unix timestamps cannot represent leap seconds. Round forward so a release
    // never becomes eligible earlier than its recorded publication time.
    Ok(nanos + i128::from(input.get(17..19) == Some("60")) * 1_000_000_000)
}

fn is_rfc3339_offset(offset: &str) -> bool {
    let bytes = offset.as_bytes();
    matches!(offset, "Z" | "z")
        || (bytes.len() == 6
            && matches!(bytes[0], b'+' | b'-')
            && bytes[3] == b':'
            && &bytes[1..3] <= b"23")
}

/// Parse registry timestamps, including UTC minute and date precision.
#[must_use]
pub fn parse_packument_timestamp(input: &str) -> Option<Timestamp> {
    if let Ok(timestamp) = parse_rfc3339(input) {
        return Some(timestamp);
    }
    let civil = DateTime::strptime("%Y-%m-%dT%H:%MZ", input)
        .ok()
        .or_else(|| {
            Date::strptime("%Y-%m-%d", input)
                .ok()
                .map(|date| date.at(0, 0, 0, 0))
        })?;
    civil
        .to_zoned(jiff::tz::TimeZone::UTC)
        .ok()
        .map(|date| date.timestamp())
}

/// Format UTC with millisecond precision, as JavaScript's `Date#toISOString`.
#[must_use]
pub fn format_millis(timestamp: Timestamp) -> String {
    DateTimePrinter::new()
        .precision(Some(3))
        .timestamp_to_string(&timestamp)
}

/// Format UTC with whole-second precision.
#[must_use]
pub fn format_seconds(timestamp: Timestamp) -> String {
    DateTimePrinter::new()
        .precision(Some(0))
        .timestamp_to_string(&timestamp)
}

/// Format UTC with a numeric offset and fractional precision in groups of three.
#[must_use]
pub fn format_rfc3339(timestamp: Timestamp) -> String {
    let nanos = timestamp.subsec_nanosecond();
    let precision = if nanos == 0 {
        0
    } else if nanos % 1_000_000 == 0 {
        3
    } else if nanos % 1_000 == 0 {
        6
    } else {
        9
    };
    let mut text = DateTimePrinter::new()
        .precision(Some(precision))
        .timestamp_to_string(&timestamp);
    text.pop();
    text.push_str("+00:00");
    text
}

/// Convert a release age in minutes without overflowing its duration.
pub fn duration_minutes(minutes: i64) -> Option<jiff::SignedDuration> {
    minutes.checked_mul(60_000).map(jiff::SignedDuration::from_millis)
}

/// Milliseconds since the Unix epoch, rounding down for dates before it.
#[must_use]
pub fn unix_millis(timestamp: Timestamp) -> i64 {
    i64::try_from(timestamp.as_nanosecond().div_euclid(1_000_000))
        .expect("timestamp milliseconds fit in i64")
}

#[cfg(test)]
mod tests;
