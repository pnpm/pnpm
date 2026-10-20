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
///Values are rounded *up* (see [`coarsen_parsed_timestamp`]) so the
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
    let keep_exact = exact_version_timestamps(time, horizon);
    let mut out = serde_json::Map::with_capacity(time.len());
    for (key, value) in time {
        let coarsened = value
            .as_str()
            .and_then(|raw| {
                let parsed = DateTime::parse_from_rfc3339(raw).ok()?.with_timezone(&Utc);
                if !is_reserved_time_key(key) && keep_exact.contains(&parsed) {
                    return None;
                }
                coarsen_parsed_timestamp(parsed, horizon).map(|(_, rendered)| rendered)
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
    horizon: DateTime<Utc>,
) -> HashSet<DateTime<Utc>> {
    let mut distinct: Vec<(DateTime<Utc>, DateTime<Utc>)> = time
        .iter()
        .filter(|(key, _)| !is_reserved_time_key(key))
        .filter_map(|(_, value)| {
            let raw = value.as_str()?;
            let parsed = DateTime::parse_from_rfc3339(raw).ok()?.with_timezone(&Utc);
            let (coarsened, _) = coarsen_parsed_timestamp(parsed, horizon)?;
            Some((parsed, coarsened))
        })
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

/// Re-render one RFC 3339 timestamp at reduced precision, **rounding
/// up**: a bare date (the next day, unless already midnight) when it
/// predates `horizon`, otherwise the next whole minute (unless already
/// on a minute boundary). Rounding up keeps the coarsened value at or
/// after the real publish time, so `minimumReleaseAge` and trust
/// checks can only ever read a version as *newer* than it is — the
/// fail-safe direction (a too-new version is never coarsened into
/// looking mature).
fn coarsen_parsed_timestamp(
    parsed: DateTime<Utc>,
    horizon: DateTime<Utc>,
) -> Option<(DateTime<Utc>, String)> {
    if parsed < horizon {
        let date = parsed.date_naive();
        let midnight = date.and_hms_opt(0, 0, 0)?.and_utc();
        let (rounded_date, rounded_dt) = if parsed == midnight {
            (date, midnight)
        } else {
            let next_date = date.succ_opt()?;
            let next_midnight = next_date.and_hms_opt(0, 0, 0)?.and_utc();
            (next_date, next_midnight)
        };
        Some((rounded_dt, rounded_date.format("%Y-%m-%d").to_string()))
    } else {
        let minute = parsed.with_second(0)?.with_nanosecond(0)?;
        let rounded = if parsed == minute { minute } else { minute + chrono::Duration::minutes(1) };
        Some((rounded, rounded.format("%Y-%m-%dT%H:%MZ").to_string()))
    }
}
