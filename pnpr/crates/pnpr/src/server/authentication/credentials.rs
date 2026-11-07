use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};

/// The pnpr token an `Authorization` header carries, in any of the shapes
/// the supported clients send: `Bearer <token>` (npm, pnpm), the bare
/// `<token>` with no scheme (`cargo`, which sends registry tokens raw), or
/// `Basic` with the token as the password (`twine` and `pip` pair it with
/// pypi.org's `__token__` pseudo-user, `docker` and `podman` with whatever
/// name the user typed at login).
///
/// The password is the whole credential: the `Basic` username is not checked
/// against the token's owner, so a caller authenticates as whoever the token
/// belongs to. That is what every registry taking a personal access token as
/// a password does, and what its clients expect.
pub(in super::super) fn token_credentials(header_value: &str) -> Option<String> {
    let value = header_value.trim();
    if let Some(bearer) = bearer_credentials(value) {
        return Some(bearer.to_string());
    }
    let Some((scheme, credentials)) = value.split_once(' ') else {
        return (!value.is_empty()).then(|| value.to_string());
    };
    if !scheme.eq_ignore_ascii_case("Basic") {
        return None;
    }
    let decoded = BASE64_STANDARD.decode(credentials.trim()).ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (_, password) = decoded.split_once(':')?;
    (!password.is_empty()).then(|| password.to_string())
}

/// The raw credentials of an `Authorization: Bearer <token>` header, or
/// `None` for any other scheme. The scheme is matched case-insensitively,
/// matching [`pnpr_auth::identify`].
pub(in super::super) fn bearer_credentials(header_value: &str) -> Option<&str> {
    let (scheme, credentials) = header_value.trim().split_once(' ')?;
    scheme.eq_ignore_ascii_case("Bearer").then(|| credentials.trim())
}
