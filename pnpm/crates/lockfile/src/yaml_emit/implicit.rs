/// Whether a plain scalar would be reinterpreted as a non-string type and so
/// must be quoted. Mirrors `testImplicitResolving` over the default schema's
/// implicit types: null, bool, int, float, timestamp, merge.
pub(super) fn resolves_implicitly(string: &str) -> bool {
    resolves_null(string)
        || resolves_bool(string)
        || resolves_int(string)
        || resolves_float(string)
        || resolves_timestamp(string)
        || string == "<<"
}

fn resolves_null(string: &str) -> bool {
    matches!(string, "~" | "null" | "Null" | "NULL")
}

fn resolves_bool(string: &str) -> bool {
    matches!(string, "true" | "True" | "TRUE" | "false" | "False" | "FALSE")
}

/// Port of `type/int.js`'s `resolveYamlInteger`.
fn resolves_int(string: &str) -> bool {
    let bytes = string.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut index = 0;
    if matches!(bytes[index], b'-' | b'+') {
        index += 1;
    }
    if index >= bytes.len() {
        return false;
    }
    if bytes[index] == b'0' {
        if index + 1 == bytes.len() {
            return true;
        }
        index += 1;
        match bytes[index] {
            b'b' => return digits_match(&bytes[index + 1..], |byte| matches!(byte, b'0' | b'1')),
            b'x' => return digits_match(&bytes[index + 1..], |byte| byte.is_ascii_hexdigit()),
            b'o' => return digits_match(&bytes[index + 1..], |byte| matches!(byte, b'0'..=b'7')),
            _ => {}
        }
    }
    if bytes[index] == b'_' {
        return false;
    }
    digits_match(&bytes[index..], |byte| byte.is_ascii_digit())
}

/// A run of `_`-separated digits accepted by `predicate`, with at least one
/// digit and no trailing `_`. Mirrors the per-base loops in `resolveYamlInteger`.
fn digits_match(bytes: &[u8], predicate: impl Fn(u8) -> bool) -> bool {
    let mut has_digits = false;
    let mut last = 0u8;
    for &byte in bytes {
        last = byte;
        if byte == b'_' {
            continue;
        }
        if !predicate(byte) {
            return false;
        }
        has_digits = true;
    }
    has_digits && last != b'_'
}

/// Port of `type/float.js`'s `YAML_FLOAT_PATTERN` test (with the trailing-`_`
/// guard).
pub(super) fn resolves_float(string: &str) -> bool {
    if string.ends_with('_') {
        return false;
    }
    float_matches(string)
}

fn float_matches(string: &str) -> bool {
    // [-+]?.inf and .nan special forms.
    let unsigned = string.strip_prefix(['-', '+']).unwrap_or(string);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") || matches!(string, ".nan" | ".NaN" | ".NAN") {
        return true;
    }

    // Form A: [0-9][0-9_]* (\.[0-9_]*)? ([eE][-+]?[0-9]+)?
    // Form B: \.[0-9_]+ ([eE][-+]?[0-9]+)?  (no leading sign per the pattern).
    match unsigned.strip_prefix('.') {
        Some(after_dot) => !string.starts_with(['-', '+']) && fraction_matches(after_dot),
        None => integer_form_matches(unsigned),
    }
}

/// Form B's body: a digit run with an optional exponent, no integer part.
fn fraction_matches(after_dot: &str) -> bool {
    let (mantissa, exponent) = split_exponent(after_dot);
    !mantissa.is_empty()
        && mantissa.bytes().all(|byte| byte.is_ascii_digit() || byte == b'_')
        && exponent_ok(exponent)
}

/// Form A's body: `[0-9][0-9_]* (\.[0-9_]*)? ([eE][-+]?[0-9]+)?`.
fn integer_form_matches(body: &str) -> bool {
    if !body.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return false;
    }
    let int_len = body.bytes().take_while(|byte| byte.is_ascii_digit() || *byte == b'_').count();
    let mut cursor = &body[int_len..];
    if let Some(rest) = cursor.strip_prefix('.') {
        let frac_len =
            rest.bytes().take_while(|byte| byte.is_ascii_digit() || *byte == b'_').count();
        cursor = &rest[frac_len..];
    }
    // ([eE][-+]?[0-9]+)? — and nothing left over.
    exponent_consumes_all(cursor)
}

/// Split a float mantissa from its optional `[eE]...` exponent.
fn split_exponent(string: &str) -> (&str, Option<&str>) {
    match string.find(['e', 'E']) {
        Some(index) => (&string[..index], Some(&string[index..])),
        None => (string, None),
    }
}

/// Whether an optional exponent tail (`""` or `[eE][-+]?[0-9]+`) is valid.
fn exponent_ok(exponent: Option<&str>) -> bool {
    match exponent {
        None | Some("") => true,
        Some(tail) => exponent_consumes_all(tail),
    }
}

fn exponent_consumes_all(tail: &str) -> bool {
    if tail.is_empty() {
        return true;
    }
    let Some(rest) = tail.strip_prefix(['e', 'E']) else {
        return false;
    };
    let rest = rest.strip_prefix(['-', '+']).unwrap_or(rest);
    !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
}

/// Port of `type/timestamp.js`'s `resolveYamlTimestamp` (date and full forms).
fn resolves_timestamp(string: &str) -> bool {
    matches_date(string) || matches_timestamp(string)
}

fn matches_date(string: &str) -> bool {
    let bytes = string.as_bytes();
    bytes.len() == 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

fn matches_timestamp(string: &str) -> bool {
    let mut scan = TimestampScan { bytes: string.as_bytes(), index: 0 };
    scan.date() && scan.time_separator() && scan.time() && scan.timezone()
}

/// A cursor over a candidate timestamp, matching js-yaml's timestamp regexp
/// one field at a time.
pub(super) struct TimestampScan<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) index: usize,
}
