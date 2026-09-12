/// Rewrite every URL in `message` so its `user:pass@` userinfo and any
/// sensitive query parameter render as `<redacted>`. Shared with the config
/// layer, whose operator-supplied endpoints can carry either.
#[must_use]
pub fn redact_url_credentials(message: &str) -> String {
    let mut redacted = String::with_capacity(message.len());
    let mut cursor = 0;
    while let Some(relative_scheme_end) = message[cursor..].find("://") {
        let scheme_end = cursor + relative_scheme_end;
        let scheme_start = find_scheme_start(message, scheme_end);
        if scheme_start == scheme_end || !is_valid_scheme(&message[scheme_start..scheme_end]) {
            redacted.push_str(&message[cursor..scheme_end + 3]);
            cursor = scheme_end + 3;
            continue;
        }

        let url_end = find_url_end(message, scheme_end + 3);
        let (candidate, suffix) = split_trailing_punctuation(&message[scheme_start..url_end]);
        let Some(safe_url) = redact_url_candidate(candidate) else {
            redacted.push_str(&message[cursor..url_end]);
            cursor = url_end;
            continue;
        };

        redacted.push_str(&message[cursor..scheme_start]);
        redacted.push_str(&safe_url);
        redacted.push_str(suffix);
        cursor = url_end;
    }
    redacted.push_str(&message[cursor..]);
    redacted
}

pub(super) fn find_scheme_start(message: &str, scheme_end: usize) -> usize {
    let bytes = message.as_bytes();
    let mut start = scheme_end;
    while start > 0 {
        let byte = bytes[start - 1];
        if !byte.is_ascii_alphanumeric() && byte != b'+' && byte != b'.' && byte != b'-' {
            break;
        }
        start -= 1;
    }
    start
}

pub(super) fn is_valid_scheme(scheme: &str) -> bool {
    let mut chars = scheme.bytes();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && chars.all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'.' || byte == b'-'
        })
}

pub(super) fn find_url_end(message: &str, url_start: usize) -> usize {
    message[url_start..]
        .char_indices()
        .find_map(|(offset, ch)| is_url_delimiter(ch).then_some(url_start + offset))
        .unwrap_or(message.len())
}

pub(super) fn is_url_delimiter(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '"' | '\'' | '`' | '<' | '>' | '(' | ')' | '{' | '}')
}

pub(super) fn split_trailing_punctuation(candidate: &str) -> (&str, &str) {
    let mut end = candidate.len();
    while let Some(ch) = candidate[..end].chars().next_back() {
        if !matches!(ch, '.' | ',' | ';' | '!') {
            break;
        }
        end -= ch.len_utf8();
    }
    (&candidate[..end], &candidate[end..])
}

pub(super) fn redact_url_candidate(candidate: &str) -> Option<String> {
    redact_parseable_url_candidate(candidate).or_else(|| redact_unparsable_url_candidate(candidate))
}

pub(super) fn redact_parseable_url_candidate(candidate: &str) -> Option<String> {
    let mut url = url::Url::parse(candidate).ok()?;
    let mut changed = redact_url_userinfo(&mut url);
    changed |= redact_url_query(&mut url);
    changed |= redact_url_fragment(&mut url);
    changed.then(|| url.to_string())
}

pub(super) fn redact_url_userinfo(url: &mut url::Url) -> bool {
    if url.username().is_empty() && url.password().is_none() {
        return false;
    }
    let username_redacted = url.set_username("redacted").is_ok();
    let password_dropped = url.set_password(None).is_ok();
    username_redacted || password_dropped
}

pub(super) fn redact_url_query(url: &mut url::Url) -> bool {
    if url.query().is_none() {
        return false;
    }
    let mut changed = false;
    let pairs = url
        .query_pairs()
        .map(|(key, value)| {
            if is_sensitive_query_key(&key) {
                changed = true;
                (key.into_owned(), "redacted".to_string())
            } else {
                (key.into_owned(), value.into_owned())
            }
        })
        .collect::<Vec<_>>();
    if changed {
        url.query_pairs_mut()
            .clear()
            .extend_pairs(pairs.iter().map(|(key, value)| (&**key, &**value)));
    }
    changed
}

pub(super) fn redact_url_fragment(url: &mut url::Url) -> bool {
    if url.fragment().is_none() {
        return false;
    }
    url.set_fragment(None);
    true
}

pub(super) fn redact_unparsable_url_candidate(candidate: &str) -> Option<String> {
    let mut redacted = candidate.to_string();
    let mut changed = false;
    if let Some(safe_url) = redact_unparsable_url_userinfo(&redacted) {
        redacted = safe_url;
        changed = true;
    }
    if let Some(safe_url) = redact_sensitive_query_values(&redacted) {
        redacted = safe_url;
        changed = true;
    }
    if let Some(safe_url) = redact_fragment(&redacted) {
        redacted = safe_url;
        changed = true;
    }
    changed.then_some(redacted)
}

pub(super) fn redact_unparsable_url_userinfo(candidate: &str) -> Option<String> {
    let authority_start = candidate.find("://")? + 3;
    let scan_end = candidate[authority_start..]
        .find('?')
        .map_or(candidate.len(), |offset| authority_start + offset);
    let userinfo_end = candidate[authority_start..scan_end].rfind('@')? + authority_start;
    let mut redacted = String::with_capacity(candidate.len());
    redacted.push_str(&candidate[..authority_start]);
    redacted.push_str("redacted@");
    redacted.push_str(&candidate[userinfo_end + 1..]);
    Some(redacted)
}

pub(super) fn redact_sensitive_query_values(candidate: &str) -> Option<String> {
    let query_start = candidate.find('?')?;
    let fragment_start = candidate[query_start + 1..]
        .find('#')
        .map_or(candidate.len(), |offset| query_start + 1 + offset);
    let query = &candidate[query_start + 1..fragment_start];
    let mut redacted = String::with_capacity(candidate.len());
    redacted.push_str(&candidate[..=query_start]);
    let mut changed = false;
    for segment in query.split_inclusive('&') {
        let (pair, separator) = segment.strip_suffix('&').map_or((segment, ""), |pair| (pair, "&"));
        if let Some(value_start) = pair.find('=') {
            let key = &pair[..value_start];
            if is_sensitive_query_key(key) {
                redacted.push_str(key);
                redacted.push_str("=redacted");
                redacted.push_str(separator);
                changed = true;
                continue;
            }
        }
        redacted.push_str(segment);
    }
    redacted.push_str(&candidate[fragment_start..]);
    changed.then_some(redacted)
}

pub(super) fn redact_fragment(candidate: &str) -> Option<String> {
    let fragment_start = candidate.find('#')?;
    Some(candidate[..fragment_start].to_string())
}

pub(super) fn is_sensitive_query_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|ch| *ch != '-' && *ch != '_')
        .map(|ch| ch.to_ascii_lowercase())
        .collect::<String>();
    matches!(
        normalized.as_str(),
        "auth"
            | "authtoken"
            | "password"
            | "passwd"
            | "pwd"
            | "secret"
            | "token"
            | "accesstoken"
            | "apikey",
    )
}
