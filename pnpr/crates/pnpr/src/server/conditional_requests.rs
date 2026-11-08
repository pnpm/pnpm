use axum::http::{HeaderMap, header};

pub(super) fn is_not_modified(
    headers: &HeaderMap,
    etag: &str,
    last_modified: Option<&str>,
) -> bool {
    let mut has_inm = false;
    for inm in headers.get_all(header::IF_NONE_MATCH) {
        has_inm = true;
        if inm.to_str().is_ok_and(|header_val| if_none_match_matches(header_val, etag)) {
            return true;
        }
    }
    if has_inm {
        return false;
    }
    if let Some(ims) = headers
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|val| val.to_str().ok())
        && let Some(lm) = last_modified
    {
        return if_modified_since_matches(ims, lm);
    }
    false
}

fn if_none_match_matches(header_val: &str, etag: &str) -> bool {
    let matches_candidate = |candidate: &str| {
        let candidate = candidate.trim();
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == etag
    };
    let mut in_quotes = false;
    let mut start = 0;
    for (idx, char_code) in header_val.char_indices() {
        match char_code {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                if matches_candidate(&header_val[start..idx]) {
                    return true;
                }
                start = idx + 1;
            }
            _ => {}
        }
    }
    matches_candidate(&header_val[start..])
}

fn if_modified_since_matches(if_modified_since: &str, last_modified: &str) -> bool {
    let Ok(ims_time) = chrono::DateTime::parse_from_rfc2822(if_modified_since) else {
        return false;
    };
    let Ok(lm_time) = chrono::DateTime::parse_from_rfc2822(last_modified) else {
        return false;
    };
    lm_time <= ims_time
}
