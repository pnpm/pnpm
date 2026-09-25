//! Choosing the `CycloneDX` representation of a package's `license` field.
//!
//! `license.id` is an enum in the `CycloneDX` schema, so a value outside the
//! SPDX license list, such as a typo or npm's own `UNLICENSED`, has to reach
//! the BOM as `license.name` or the whole document fails validation.
//!
//! SPDX 2.3 annex D.2 matches license and exception identifiers
//! case-insensitively and requires the operators to be uppercase. The `spdx`
//! crate has it the other way around, so identifiers are rewritten to their
//! canonical case and lowercase operators rejected before it parses an
//! expression.

pub(super) fn classify_license(license: &str) -> serde_json::Value {
    if let Some(id) = canonical_spdx_id(license.trim_matches(' ')) {
        serde_json::json!({ "license": { "id": id } })
    } else if is_spdx_expression(license) {
        serde_json::json!({ "expression": license })
    } else {
        serde_json::json!({ "license": { "name": license } })
    }
}

/// The canonically cased SPDX identifier `license` names, if it names one.
///
/// A trailing `+` makes the value an expression rather than an identifier, so
/// it is left to [`is_spdx_expression`]; `spdx::license_id` would otherwise
/// strip it and report the base license.
fn canonical_spdx_id(license: &str) -> Option<&'static str> {
    if license.ends_with('+') {
        return None;
    }
    let canonical = spdx::license_id(license)
        .map(|id| id.name)
        .or_else(|| case_insensitive_license_id(license))?;
    (!is_unlisted_spdx_name(canonical)).then_some(canonical)
}

fn case_insensitive_license_id(license: &str) -> Option<&'static str> {
    spdx::identifiers::LICENSES
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(license))
        .map(|candidate| candidate.name)
}

/// Names the `spdx` crate resolves that are not SPDX license identifiers: the
/// document keywords `NONE` and `NOASSERTION`, and the synthetic base names it
/// lists for the GNU licenses, such as `GFDL-1.1-invariants`, so that it can
/// build their `-only` and `-or-later` forms.
fn is_unlisted_spdx_name(value: &str) -> bool {
    ["NONE", "NOASSERTION"]
        .iter()
        .any(|keyword| value.eq_ignore_ascii_case(keyword))
        || spdx::license_id(value).is_some_and(is_gnu_base_name)
}

/// A bare GNU name that the SPDX license list itself never carried. Every GNU
/// identifier it did carry bare is deprecated in favor of the `-only` and
/// `-or-later` forms.
fn is_gnu_base_name(id: spdx::LicenseId) -> bool {
    id.is_gnu()
        && !id.is_deprecated()
        && !id.name.ends_with("-only")
        && !id.name.ends_with("-or-later")
}

fn is_spdx_expression(license: &str) -> bool {
    let Some(normalized) = normalize_spdx_expression_ids(license) else {
        return false;
    };
    spdx::Expression::parse_mode(
        &normalized,
        spdx::ParseMode {
            allow_deprecated: true,
            allow_postfix_plus_on_gpl: true,
            ..spdx::ParseMode::STRICT
        },
    )
    .is_ok()
}

/// `expression` with every identifier in its canonical case, or `None` when it
/// holds a token the SPDX 2.3 grammar has no place for.
fn normalize_spdx_expression_ids(expression: &str) -> Option<String> {
    let mut normalized = String::with_capacity(expression.len());
    let mut token_start = 0;
    for (index, ch) in expression.char_indices() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.') {
            continue;
        }
        // The `spdx` crate skips every Unicode whitespace character, while an
        // SPDX 2.3 expression is one line and the v11 parser skips only the
        // ASCII space.
        if ch.is_whitespace() && ch != ' ' {
            return None;
        }
        push_normalized_spdx_token(&mut normalized, &expression[token_start..index])?;
        normalized.push(ch);
        token_start = index + ch.len_utf8();
    }
    push_normalized_spdx_token(&mut normalized, &expression[token_start..])?;
    Some(normalized)
}

/// `AdditionRef-` is SPDX 3.0 syntax that the `spdx` crate accepts and the
/// SPDX 2.3 grammar `CycloneDX` documents does not.
fn push_normalized_spdx_token(normalized: &mut String, token: &str) -> Option<()> {
    if token.starts_with("AdditionRef-") || is_unlisted_spdx_name(token) {
        return None;
    }
    if is_lowercased_spdx_operator(token) {
        return None;
    }
    let token =
        canonical_spdx_id(token).or_else(|| canonical_spdx_exception_id(token)).unwrap_or(token);
    normalized.push_str(token);
    Some(())
}

fn is_lowercased_spdx_operator(token: &str) -> bool {
    ["AND", "OR", "WITH"]
        .iter()
        .any(|operator| token.eq_ignore_ascii_case(operator) && token != *operator)
}

fn canonical_spdx_exception_id(exception: &str) -> Option<&'static str> {
    spdx::exception_id(exception)
        .map(|id| id.name)
        .or_else(|| {
            spdx::identifiers::EXCEPTIONS
                .iter()
                .find(|candidate| candidate.name.eq_ignore_ascii_case(exception))
                .map(|candidate| candidate.name)
        })
}
