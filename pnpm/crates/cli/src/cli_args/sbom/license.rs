pub(super) fn classify_license(license: &str) -> serde_json::Value {
    if let Some(id) = canonical_spdx_id(license) {
        serde_json::json!({ "license": { "id": id } })
    } else if is_spdx_expression(license) {
        serde_json::json!({ "expression": license })
    } else {
        serde_json::json!({ "license": { "name": license } })
    }
}

fn canonical_spdx_id(license: &str) -> Option<&'static str> {
    if license.ends_with('+') || is_spdx_document_value(license) {
        return None;
    }
    spdx::license_id(license)
        .map(|id| id.name)
        .or_else(|| {
            spdx::identifiers::LICENSES
                .iter()
                .find(|id| id.name.eq_ignore_ascii_case(license))
                .map(|id| id.name)
        })
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

fn normalize_spdx_expression_ids(expression: &str) -> Option<String> {
    let mut normalized = String::with_capacity(expression.len());
    let mut token_start = 0;
    for (index, ch) in expression.char_indices() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.') {
            continue;
        }
        push_normalized_spdx_token(&mut normalized, &expression[token_start..index])?;
        normalized.push(ch);
        token_start = index + ch.len_utf8();
    }
    push_normalized_spdx_token(&mut normalized, &expression[token_start..])?;
    Some(normalized)
}

fn push_normalized_spdx_token(normalized: &mut String, token: &str) -> Option<()> {
    if token.starts_with("AdditionRef-") || is_spdx_document_value(token) {
        return None;
    }
    if ["AND", "OR", "WITH"]
        .iter()
        .any(|operator| token.eq_ignore_ascii_case(operator) && token != *operator)
    {
        return None;
    }
    let token =
        canonical_spdx_id(token).or_else(|| canonical_spdx_exception_id(token)).unwrap_or(token);
    normalized.push_str(token);
    Some(())
}

fn is_spdx_document_value(value: &str) -> bool {
    ["NONE", "NOASSERTION"]
        .iter()
        .any(|item| value.eq_ignore_ascii_case(item))
}

fn canonical_spdx_exception_id(exception: &str) -> Option<&'static str> {
    spdx::exception_id(exception)
        .map(|id| id.name)
        .or_else(|| {
            spdx::identifiers::EXCEPTIONS
                .iter()
                .find(|id| id.name.eq_ignore_ascii_case(exception))
                .map(|id| id.name)
        })
}
