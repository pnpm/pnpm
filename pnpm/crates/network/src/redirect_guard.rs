use std::{collections::HashSet, sync::Arc};

use crate::origin_of_url;

/// A redirect-hop validator: returns `true` to follow a redirect to `url`,
/// `false` to block it. See
/// [`ThrottledClient::new_for_installs_with_redirect_guard`](crate::ThrottledClient::new_for_installs_with_redirect_guard).
pub type RedirectGuard = Arc<dyn Fn(&reqwest::Url) -> bool + Send + Sync>;

/// Cap on redirect hops, matching reqwest's default `Policy::default()` limit
/// so the guarded client doesn't follow a redirect chain further than the
/// unguarded one would.
pub(crate) const MAX_REDIRECT_HOPS: usize = 10;

/// A redirect target the [`RedirectGuard`] rejected. Surfaced as the request
/// error so a blocked redirect fails loudly rather than silently fetching.
#[derive(Debug)]
pub(crate) struct BlockedRedirect(pub(crate) reqwest::Url);

impl std::fmt::Display for BlockedRedirect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Surface only `scheme://host[:port]` — never the path, query,
        // fragment, or userinfo, where a presigned-URL signature/token could
        // live. This error string can reach a client, so it must not leak the
        // very credential the redirect was carrying.
        write!(
            f,
            "redirect to {}://{}",
            self.0.scheme(),
            self.0.host_str().unwrap_or("<unknown>"),
        )?;
        if let Some(port) = self.0.port() {
            write!(f, ":{port}")?;
        }
        write!(f, " is not allowed by the fetch allowlist")
    }
}

impl std::error::Error for BlockedRedirect {}

/// Create a [`RedirectGuard`] that allows redirects only to origins matching the
/// supplied registry URLs (comparing scheme, host, and port).
#[must_use]
pub fn origins_redirect_guard<'a, Urls>(allowed_urls: Urls) -> RedirectGuard
where
    Urls: IntoIterator<Item = &'a str>,
{
    let origins: HashSet<String> = allowed_urls
        .into_iter()
        .filter_map(|url| reqwest::Url::parse(url).ok().and_then(|u| origin_of_url(&u)))
        .collect();
    Arc::new(move |target: &reqwest::Url| -> bool {
        origin_of_url(target).is_some_and(|origin| origins.contains(&origin))
    })
}

#[cfg(test)]
mod tests;
