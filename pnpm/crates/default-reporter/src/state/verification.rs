use crate::format::pretty_ms_compact;
use chrono::{DateTime, Utc};

pub(super) fn entries_label(entries: u64) -> String {
    if entries == 1 {
        "1 entry".to_string()
    } else {
        format!("{entries} entries")
    }
}

/// How a cache-satisfied verification verdict is dated: relative to `now`
/// when the record carries a parseable timestamp, timeless otherwise. The
/// age is clamped at zero so a clock that moved backwards between the
/// verification run and this install cannot render a negative age.
pub(super) fn cached_verdict(verified_at: Option<&str>, now: DateTime<Utc>) -> String {
    let elapsed_ms = verified_at
        .and_then(|verified_at| DateTime::parse_from_rfc3339(verified_at).ok())
        .map(|verified_at| (now - verified_at.with_timezone(&Utc)).num_milliseconds().max(0));
    match elapsed_ms {
        Some(elapsed_ms) => {
            format!(
                "verified {} ago",
                pretty_ms_compact(elapsed_ms.unsigned_abs().into()),
            )
        }
        None => "previously verified".to_string(),
    }
}
