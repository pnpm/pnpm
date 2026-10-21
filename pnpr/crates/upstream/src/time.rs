use std::collections::HashSet;

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
/// Values are rounded *up* so the
/// maturity- and trust-checks that read them stay fail-safe. Version
/// entries whose coarsened timestamps would collide with or cross an
/// adjacent version's publish time retain their exact RFC 3339 strings
/// so `trustPolicy: no-downgrade` preserves strict chronological order
/// across same-day or same-minute releases. Non-timestamp entries (the
/// reserved `unpublished` object) and any value pnpr can't parse as
/// RFC 3339 pass through untouched.
pub fn coarsen_time_map(
    time: &serde_json::Map<String, Value>,
    now: DateTime<Utc>,
) -> serde_json::Map<String, Value> {
    let horizon = now - chrono::Duration::days(TIME_PRECISION_HORIZON_DAYS);
    let parsed_entries: Vec<Option<(DateTime<Utc>, DateTime<Utc>)>> = time
        .values()
        .map(|value| {
            let raw = value.as_str()?;
            let parsed = DateTime::parse_from_rfc3339(raw).ok()?.with_timezone(&Utc);
            let rounded = coarsen_timestamp_dt(parsed, horizon)?;
            Some((parsed, rounded))
        })
        .collect();
    let keep_exact = exact_version_timestamps(time, &parsed_entries);
    let mut out = serde_json::Map::with_capacity(time.len());
    for ((key, value), parsed) in time.iter().zip(parsed_entries) {
        let coarsened = parsed.and_then(|(parsed, rounded)| {
            if !is_reserved_time_key(key) && keep_exact.contains(&parsed) {
                return None;
            }
            Some(format_coarsened_timestamp(parsed, rounded, horizon))
        });
        out.insert(key.clone(), coarsened.map_or_else(|| value.clone(), Value::String));
    }
    out
}

fn is_reserved_time_key(key: &str) -> bool {
    matches!(key, "created" | "modified" | "unpublished")
}

fn exact_version_timestamps(
    time: &serde_json::Map<String, Value>,
    parsed_entries: &[Option<(DateTime<Utc>, DateTime<Utc>)>],
) -> HashSet<DateTime<Utc>> {
    let mut distinct: Vec<(DateTime<Utc>, DateTime<Utc>)> = time
        .keys()
        .zip(parsed_entries.iter().copied())
        .filter(|(key, _)| !is_reserved_time_key(key))
        .filter_map(|(_, parsed)| parsed)
        .collect();
    distinct.sort_unstable_by_key(|(parsed, _)| *parsed);
    distinct.dedup_by_key(|(parsed, _)| *parsed);

    let mut keep_exact = HashSet::new();
    for pair in distinct.windows(2) {
        let [(earlier, earlier_coarsened), (later, _)] = pair else { continue };
        if earlier_coarsened >= later {
            keep_exact.insert(*earlier);
            keep_exact.insert(*later);
        }
    }
    keep_exact
}

/// Round one RFC 3339 timestamp **up** to reduced precision: midnight of
/// the next day (unless already midnight) when it predates `horizon`,
/// otherwise the next whole minute (unless already on a minute boundary).
/// Rounding up keeps the coarsened value at or after the real publish
/// time, so `minimumReleaseAge` and trust checks can only ever read a
/// version as *newer* than it is — the fail-safe direction (a too-new
/// version is never coarsened into looking mature).
fn coarsen_timestamp_dt(parsed: DateTime<Utc>, horizon: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if parsed < horizon {
        let date = parsed.date_naive();
        let midnight = date.and_hms_opt(0, 0, 0)?.and_utc();
        if parsed == midnight {
            Some(midnight)
        } else {
            date.succ_opt()?
                .and_hms_opt(0, 0, 0)
                .map(|dt| dt.and_utc())
        }
    } else {
        let minute = parsed.with_second(0)?.with_nanosecond(0)?;
        Some(if parsed == minute { minute } else { minute + chrono::Duration::minutes(1) })
    }
}

fn format_coarsened_timestamp(
    parsed: DateTime<Utc>,
    rounded: DateTime<Utc>,
    horizon: DateTime<Utc>,
) -> String {
    if parsed < horizon {
        rounded
            .date_naive()
            .format("%Y-%m-%d")
            .to_string()
    } else {
        rounded.format("%Y-%m-%dT%H:%MZ").to_string()
    }
}
