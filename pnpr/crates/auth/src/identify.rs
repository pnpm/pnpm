use super::{Result, TokenBackend};

/// Identify the caller behind an HTTP request. Inspects the
/// `Authorization` header and resolves it to a username via the token
/// store. Only `Bearer` tokens are honored: pnpr does not accept Basic
/// credentials on requests.
///
/// The scheme is matched case-insensitively (RFC 7235 §2.1: "the
/// scheme is case-insensitive"), so `BEARER`, `bearer`, and `Bearer`
/// all parse the same.
/// `Ok(None)` covers every "no usable credentials" case — a missing or
/// malformed header, a non-`Bearer` scheme (including legacy `Basic`), or a
/// token that doesn't match. `Err` is reserved for a failure of the
/// backing store (e.g. the networked auth DB is unreachable) so the caller
/// can return a 5xx instead of a misleading 401.
pub async fn identify(
    header_value: Option<&str>,
    tokens: &dyn TokenBackend,
) -> Result<Option<String>> {
    let Some(value) = header_value.map(str::trim) else {
        return Ok(None);
    };
    let mut parts = value.splitn(2, ' ');
    let Some(scheme) = parts.next() else {
        return Ok(None);
    };
    let Some(credentials) = parts.next().map(str::trim) else {
        return Ok(None);
    };
    if scheme.eq_ignore_ascii_case("Bearer") {
        return tokens.lookup(credentials).await;
    }
    Ok(None)
}
