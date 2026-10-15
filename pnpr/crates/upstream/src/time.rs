use chrono::{DateTime, Timelike, Utc};
use serde_json::Value;

pub const TIME_PRECISION_HORIZON_DAYS: i64 = 7;

/// Shrink a packument `time` map by dropping precision the resolvers
/// don't need: seconds come off every timestamp, and entries older
/// than [`TIME_PRECISION_HORIZON_DAYS`] lose the time-of-day entirely
/// (down to the bare `YYYY-MM-DD`). Responses go out uncompressed, so
/// every character dropped is a byte off the wire.
///
/// Both reduced forms stay parseable by pnpm (`new Date`) and pacquet
/// (`pnpm_resolving_resolver_base::parse_packument_timestamp`).
/// Values are rounded *up* (see [`coarsen_timestamp`]) so the
/// maturity- and trust-checks that read them stay fail-safe.
/// Non-timestamp entries (the reserved `unpublished` object) and any
/// value pnpr can't parse as RFC 3339 pass through untouched.
pub fn coarsen_time_map(
    time: &serde_json::Map<String, Value>,
    now: DateTime<Utc>,
) -> serde_json::Map<String, Value> {
    let horizon = now - chrono::Duration::days(TIME_PRECISION_HORIZON_DAYS);
    let mut out = serde_json::Map::with_capacity(time.len());
    for (key, value) in time {
        let coarsened = value.as_str().and_then(|raw| coarsen_timestamp(raw, horizon));
        out.insert(key.clone(), coarsened.map_or_else(|| value.clone(), Value::String));
    }
    out
}

/// Re-render one RFC 3339 timestamp at reduced precision, **rounding
/// up**: a bare date (the next day, unless already midnight) when it
/// predates `horizon`, otherwise the next whole minute (unless already
/// on a minute boundary). Rounding up keeps the coarsened value at or
/// after the real publish time, so `minimumReleaseAge` and trust
/// checks can only ever read a version as *newer* than it is — the
/// fail-safe direction (a too-new version is never coarsened into
/// looking mature). Returns `None` for strings that aren't RFC 3339 so
/// the caller keeps the original verbatim.
pub fn coarsen_timestamp(raw: &str, horizon: DateTime<Utc>) -> Option<String> {
    let parsed = DateTime::parse_from_rfc3339(raw).ok()?.with_timezone(&Utc);
    if parsed < horizon {
        let date = parsed.date_naive();
        let rounded =
            if parsed == date.and_hms_opt(0, 0, 0)?.and_utc() { date } else { date.succ_opt()? };
        Some(rounded.format("%Y-%m-%d").to_string())
    } else {
        let minute = parsed.with_second(0)?.with_nanosecond(0)?;
        let rounded = if parsed == minute { minute } else { minute + chrono::Duration::minutes(1) };
        Some(rounded.format("%Y-%m-%dT%H:%MZ").to_string())
    }
}
