use super::redact_and_sanitize;

/// Whether the authority of `url` carries a `user:pass@` prefix. The authority
/// ends at the first `/`, `?`, or `#`, so a later `@` in the path is not one.
///
/// Both the full form and the scheme-less `//host/` form count. The latter is
/// the shape `.npmrc` scopes settings with, so it is the one a user is most
/// likely to reach for here.
pub(super) fn registry_url_has_userinfo(url: &str) -> bool {
    userinfo_end(url).is_some()
}

/// The offset just past the `user:pass@` of `url`, or [`None`] when its
/// authority carries none. Splitting it out keeps the detection and the
/// redaction below agreeing on what the authority is.
fn userinfo_end(url: &str) -> Option<usize> {
    let authority_start = authority_start_of(url)?;
    let authority = &url[authority_start..];
    let authority_end = authority.find(['/', '?', '#']).unwrap_or(authority.len());
    authority[..authority_end].rfind('@').map(|at| authority_start + at + 1)
}

/// Where the authority of `url` begins, or [`None`] if it has none.
///
/// The scheme is anchored at the start rather than found by searching for the
/// first `://`: a `://` inside the path (`//host/a://b`) would otherwise be
/// taken for the separator, and the real authority — credentials and all —
/// would go unexamined.
fn authority_start_of(url: &str) -> Option<usize> {
    if let Some(scheme_end) = url.find("://") {
        let scheme = &url[..scheme_end];
        let mut chars = scheme.chars();
        let starts_with_letter = chars.next().is_some_and(|first| first.is_ascii_alphabetic());
        let rest_is_scheme = chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '.' | '-')
        });
        if starts_with_letter && rest_is_scheme {
            return Some(scheme_end + "://".len());
        }
    }
    url.starts_with("//").then_some("//".len())
}

/// `url` with any `user:pass@` removed, safe to put in a message.
///
/// [`redact_and_sanitize`] only recognizes an authority after a `://`, and
/// deliberately so: it runs over arbitrary prose, where a bare `//` is more
/// often a comment or a path than a URL. Here the string is known to be a
/// registry URL, so the scheme-less `//host/` form can be handled too.
pub(super) fn redact_registry_url(url: &str) -> String {
    match (authority_start_of(url), userinfo_end(url)) {
        (Some(authority_start), Some(userinfo_end)) => {
            redact_and_sanitize(&format!("{}{}", &url[..authority_start], &url[userinfo_end..]))
        }
        _ => redact_and_sanitize(url),
    }
}
