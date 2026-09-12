/// Whether a registry/dependency/tarball spec carries inline
/// `user:pass@host` (or `user@host`) credentials. Such URLs must be
/// rejected before any fetch: pnpr must not turn a client-embedded
/// credential into upstream Basic auth, treat it as a cache identity, or
/// store it in a shared cache. An ssh remote's bare username
/// (`git+ssh://git@host/...`) is transport addressing with no secret — the
/// same login the scp form `git@host:path` spells — so on ssh schemes only
/// a `user:pass@` userinfo counts. Specs without a `scheme://` (a semver
/// range, a scoped name, `npm:`/`workspace:` aliases) never match.
#[must_use]
pub fn url_has_inline_credentials(spec: &str) -> bool {
    let Some((scheme, after_scheme)) = spec.split_once("://") else {
        return false;
    };
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or(after_scheme);
    let Some((userinfo, _)) = authority.rsplit_once('@') else {
        return false;
    };
    if scheme.eq_ignore_ascii_case("ssh") || scheme.eq_ignore_ascii_case("git+ssh") {
        return userinfo.contains(':');
    }
    !userinfo.is_empty()
}

/// Strip an inline `user:pass@`/`user@` userinfo from a `scheme://` URL,
/// returning the credential-free form. A tarball URL taken from an untrusted
/// upstream `dist.tarball` must never be emitted to a client or written to a
/// shared cache with embedded credentials; a genuinely public tarball is
/// anonymously fetchable, so the stripped URL still works. Returns the input
/// unchanged when there is no `scheme://` authority or no userinfo.
#[must_use]
pub fn strip_url_credentials(url: &str) -> String {
    let Some((scheme, after)) = url.split_once("://") else {
        return url.to_string();
    };
    let authority_end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let (authority, rest) = after.split_at(authority_end);
    match authority.rsplit_once('@') {
        Some((_, host)) => format!("{scheme}://{host}{rest}"),
        None => url.to_string(),
    }
}

/// Sanitize an upstream `dist.tarball` URL for public emission: drop inline
/// userinfo (via [`strip_url_credentials`]) **and** any query string or
/// fragment, where a registry could carry a signed-URL / tokenized credential
/// (`?X-Amz-Signature=…`, `?token=…`). A genuinely public tarball is fetched by
/// its bare path and verified by SRI regardless, so the sanitized URL still
/// works; one that truly needed a token was never a public route and must be
/// configured as an upstream (whose tarballs route through `/~<name>/`, keeping
/// the token server-side). Unlike a client-supplied direct-tarball spec — whose
/// query is the caller's own intent — this is untrusted upstream metadata.
#[must_use]
pub fn sanitize_registry_tarball_url(url: &str) -> String {
    let no_creds = strip_url_credentials(url);
    match no_creds.split_once(['?', '#']) {
        Some((base, _)) => base.to_string(),
        None => no_creds,
    }
}

pub(super) fn scheme_of(url: &str) -> Option<&str> {
    url.split_once("://").map(|(scheme, _)| scheme)
}
