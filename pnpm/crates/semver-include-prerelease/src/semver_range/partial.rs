use std::borrow::Cow;

use node_semver::{MAX_LENGTH, MAX_SAFE_INTEGER};

const MAX_SAFE_BUILD_LENGTH: usize = MAX_LENGTH - 6;

pub(super) enum FastToken {
    Reject,
    Any,
    Direct,
    Desugar,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Component<'a> {
    Digits(&'a str),
    Wildcard,
}

pub(super) struct PartialVersion<'a> {
    pub(super) major: Component<'a>,
    pub(super) minor: Option<Component<'a>>,
    pub(super) patch: Option<Component<'a>>,
    pub(super) pre: Option<&'a str>,
}

pub(super) fn classify_fast_token(range: &str, is_ws: fn(char) -> bool) -> FastToken {
    if range.is_empty() || range == "*" || range == ">=0.0.0" {
        return FastToken::Any;
    }
    if !starts_with_semver_char(range, is_ws) || range.bytes().any(is_non_semver_byte) {
        return FastToken::Reject;
    }
    if range.bytes().any(needs_desugar_scan) || range.starts_with("~>") {
        return FastToken::Desugar;
    }
    let (_, after_op) = strip_fast_operator(range);
    if after_op.is_empty() || after_op.starts_with('=') {
        return FastToken::Desugar;
    }
    let main = after_op
        .split('-')
        .next()
        .unwrap_or(after_op);
    if main
        .bytes()
        .filter(|&byte| byte == b'.')
        .count()
        < 2
    {
        return FastToken::Desugar;
    }
    classify_direct_triple(after_op)
}

fn starts_with_semver_char(range: &str, is_ws: fn(char) -> bool) -> bool {
    range
        .chars()
        .next()
        .is_some_and(|ch| {
            ch.is_ascii_digit()
                || matches!(ch, '^' | '~' | '<' | '>' | '=' | 'v' | 'x' | 'X' | '*' | '+' | '|')
                || is_ws(ch)
        })
}

fn is_non_semver_byte(byte: u8) -> bool {
    matches!(byte, b':' | b'/' | b'\\' | b'@')
}

fn needs_desugar_scan(byte: u8) -> bool {
    byte >= 0x80 || byte.is_ascii_whitespace() || matches!(byte, b'|' | b'+' | b'x' | b'X' | b'*')
}

fn strip_fast_operator(text: &str) -> (&str, &str) {
    for prefix in ["^", "~", ">=", "<=", ">", "<", "="] {
        if let Some(after) = text.strip_prefix(prefix) {
            return (prefix, after);
        }
    }
    ("", text)
}

fn classify_direct_triple(ver: &str) -> FastToken {
    if ver.len() > MAX_LENGTH {
        return FastToken::Reject;
    }
    let after_v = ver.strip_prefix('v').unwrap_or(ver);
    let (main, pre) = match after_v.split_once('-') {
        Some((main, pre)) if validate_prerelease(pre).is_some() => (main, Some(pre)),
        Some(_) => return FastToken::Reject,
        None => (after_v, None),
    };
    let _ = pre;
    let Some((major, minor, patch)) = parse_strict_triple(main) else {
        return FastToken::Reject;
    };
    if major < MAX_SAFE_INTEGER && minor < MAX_SAFE_INTEGER && patch < MAX_SAFE_INTEGER {
        FastToken::Direct
    } else {
        FastToken::Desugar
    }
}

pub(super) fn parse_xrange_plain(text: &str) -> Option<PartialVersion<'_>> {
    let stripped = text.trim_start_matches(['v', '=', ' ']);
    let (core, pre) = match stripped.split_once('-') {
        Some((core, pre)) => (core, Some(validate_prerelease(pre)?)),
        None => (stripped, None),
    };
    let mut parts = core.split('.');
    let major = parse_xrange_component(parts.next()?)?;
    let minor = match parts.next() {
        Some(part) => Some(parse_xrange_component(part)?),
        None => None,
    };
    let patch = match parts.next() {
        Some(part) => Some(parse_xrange_component(part)?),
        None => None,
    };
    if parts.next().is_some() || (pre.is_some() && patch.is_none()) {
        return None;
    }
    Some(PartialVersion { major, minor, patch, pre })
}

pub(super) fn invalid_xrange_order(version: &PartialVersion<'_>) -> bool {
    matches!(
        (version.major, version.minor, version.patch),
        (Component::Wildcard, Some(Component::Digits(_)), _)
            | (_, None | Some(Component::Wildcard), Some(Component::Digits(_))),
    )
}

pub(super) fn inc_num(digits: &str) -> Option<u64> {
    digits
        .parse::<u64>()
        .ok()?
        .checked_add(1)
        .filter(|&value| value <= MAX_SAFE_INTEGER)
}

pub(super) fn canonical_strict_comparator(token: &str) -> Option<String> {
    if token.is_empty() {
        return Some(String::new());
    }
    if token == "<0.0.0-0" {
        return Some(token.to_string());
    }
    let (op, after_op) = strip_gtlt_operator(token);
    let canonical_op = if op == "=" { "" } else { op };
    let ver = after_op.strip_prefix(' ').unwrap_or(after_op);
    if ver.is_empty() || ver.len() > MAX_LENGTH {
        return None;
    }
    let after_v = ver.strip_prefix('v').unwrap_or(ver);
    let (main, pre) = match after_v.split_once('-') {
        Some((main, pre)) => (main, Some(validate_prerelease(pre)?)),
        None => (after_v, None),
    };
    let (major, minor, patch) = parse_strict_triple(main)?;
    match pre {
        Some(pre) => Some(format!("{canonical_op}{major}.{minor}.{patch}-{pre}")),
        None => Some(format!("{canonical_op}{major}.{minor}.{patch}")),
    }
}

pub(super) fn strip_gtlt_operator(text: &str) -> (&str, &str) {
    for prefix in [">=", "<=", ">", "<", "="] {
        if let Some(after) = text.strip_prefix(prefix) {
            return (prefix, after);
        }
    }
    ("", text)
}

pub(super) fn strip_build_substrings(text: &str) -> Cow<'_, str> {
    if !text.contains('+') {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'+'
            && let Some(end) = consume_build_run(bytes, idx + 1)
        {
            idx = end;
            continue;
        }
        let ch = text[idx..]
            .chars()
            .next()
            .expect("idx < bytes.len()");
        out.push(ch);
        idx += ch.len_utf8();
    }
    Cow::Owned(out)
}

pub(super) fn trim_comparator_whitespace(text: &str) -> Cow<'_, str> {
    if !text.contains(' ') {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copy_from = 0;
    let mut cursor = 0;
    while cursor < bytes.len() {
        let (removable_ws, next_cursor) = step_comparator_candidate(bytes, cursor);
        if let Some(ws_pos) = removable_ws {
            out.push_str(&text[copy_from..ws_pos]);
            copy_from = ws_pos + 1;
        }
        cursor = next_cursor;
    }
    if copy_from == 0 {
        return Cow::Borrowed(text);
    }
    out.push_str(&text[copy_from..]);
    Cow::Owned(out)
}

fn step_comparator_candidate(bytes: &[u8], match_start: usize) -> (Option<usize>, usize) {
    let first = bytes[match_start];
    let after_angle = if matches!(first, b'<' | b'>') {
        match_start + 1
    } else if is_version_prefix(first) || is_version_start(first) {
        match_start
    } else {
        return (None, match_start + 1);
    };
    let ver_start = skip_version_prefixes(bytes, after_angle);
    if !bytes
        .get(ver_start)
        .copied()
        .is_some_and(is_version_start)
    {
        return (None, ver_start);
    }
    let removable_ws = removable_whitespace_index(bytes, match_start);
    (removable_ws, advance_past_version_candidate(bytes, ver_start))
}

fn skip_version_prefixes(bytes: &[u8], mut cursor: usize) -> usize {
    while cursor < bytes.len() && is_version_prefix(bytes[cursor]) {
        cursor += 1;
    }
    cursor
}

fn removable_whitespace_index(bytes: &[u8], match_start: usize) -> Option<usize> {
    let mut pos = match_start;
    if bytes.get(pos) == Some(&b' ') {
        pos += 1;
    }
    if matches!(bytes.get(pos), Some(b'<' | b'>')) {
        pos += 1;
    }
    if bytes.get(pos) == Some(&b'=') {
        pos += 1;
    }
    (bytes.get(pos) == Some(&b' ')).then_some(pos)
}

fn advance_past_version_candidate(bytes: &[u8], mut cursor: usize) -> usize {
    while cursor < bytes.len()
        && (bytes[cursor].is_ascii_alphanumeric()
            || matches!(bytes[cursor], b'.' | b'-' | b'+' | b'*'))
    {
        cursor += 1;
    }
    cursor
}

fn is_version_prefix(byte: u8) -> bool {
    matches!(byte, b'v' | b'=' | b' ')
}

fn is_version_start(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'x' | b'X' | b'*')
}

fn parse_xrange_component(part: &str) -> Option<Component<'_>> {
    if matches!(part, "x" | "X" | "*") {
        return Some(Component::Wildcard);
    }
    is_strict_digits(part).then_some(Component::Digits(part))
}

fn parse_strict_triple(main: &str) -> Option<(u64, u64, u64)> {
    let mut parts = main.split('.');
    let major = parse_bounded_uint(parts.next()?)?;
    let minor = parse_bounded_uint(parts.next()?)?;
    let patch = parse_bounded_uint(parts.next()?)?;
    parts
        .next()
        .is_none()
        .then_some((major, minor, patch))
}

fn parse_bounded_uint(part: &str) -> Option<u64> {
    if !is_strict_digits(part) {
        return None;
    }
    part.parse::<u64>()
        .ok()
        .filter(|&value| value <= MAX_SAFE_INTEGER)
}

fn is_strict_digits(part: &str) -> bool {
    if part == "0" {
        return true;
    }
    part.len() <= MAX_LENGTH + 1
        && part.starts_with(|ch: char| ch.is_ascii_digit() && ch != '0')
        && part.bytes().all(|byte| byte.is_ascii_digit())
}

fn validate_prerelease(prerelease: &str) -> Option<&str> {
    (!prerelease.is_empty() && prerelease.split('.').all(is_prerelease_identifier)).then_some(
        prerelease,
    )
}

fn is_prerelease_identifier(ident: &str) -> bool {
    if ident.is_empty() || !ident.bytes().all(is_ident_byte) {
        return false;
    }
    let Some(first_non_digit) = ident
        .bytes()
        .position(|byte| !byte.is_ascii_digit())
    else {
        return is_strict_digits(ident);
    };
    first_non_digit <= MAX_LENGTH && ident.len() - (first_non_digit + 1) <= MAX_SAFE_BUILD_LENGTH
}

fn consume_build_run(bytes: &[u8], start: usize) -> Option<usize> {
    let mut end = consume_ident_run(bytes, start)?;
    while bytes.get(end) == Some(&b'.')
        && let Some(next_end) = consume_ident_run(bytes, end + 1)
    {
        end = next_end;
    }
    Some(end)
}

fn consume_ident_run(bytes: &[u8], start: usize) -> Option<usize> {
    let mut end = start;
    while bytes
        .get(end)
        .copied()
        .is_some_and(is_ident_byte)
    {
        end += 1;
    }
    (end > start).then_some(end)
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}
