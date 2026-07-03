//! Proxy configuration types and helpers consumed by
//! [`crate::ThrottledClient::for_installs`].
//!
//! [`ProxyConfig`] holds the resolved `(https_proxy, http_proxy, no_proxy)`
//! triple — typically built by `pacquet-config` from the `.npmrc` keys
//! `https-proxy`, `http-proxy`, `proxy` (legacy), `no-proxy` and
//! `noproxy`, plus the env-var fallback cascade. [`NoProxyMatcher`] and
//! the URL helpers are private to the crate; they're invoked from the
//! client constructor.

use derive_more::{Display, Error};
use miette::Diagnostic;
use reqwest::Url;

/// Resolved proxy configuration after the `.npmrc` + env cascade has run.
///
/// All three fields are `None` when no proxy is configured. Built once
/// inside `pacquet_config::Config::current` and threaded into the
/// install client by [`crate::ThrottledClient::for_installs`]. Lives in
/// `pacquet-network` (rather than `pacquet-config`) because
/// `pacquet-config` already depends on `pacquet-network` for the auth
/// plumbing, so adding the reverse direction would form a cycle.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProxyConfig {
    /// Proxy URL used for HTTPS targets. `None` means no proxy. May carry
    /// userinfo (`http://user:pass@host:port`) which the network layer
    /// strips and percent-decodes before forming the
    /// `Proxy-Authorization` header.
    pub https_proxy: Option<String>,

    /// Proxy URL used for HTTP targets. Falls back through the cascade to
    /// `https_proxy` when no explicit `http-proxy` / env var is set.
    pub http_proxy: Option<String>,

    /// Hosts that should bypass any configured proxy. `None` = no bypass
    /// rules; [`NoProxySetting::Bypass`] = bypass every proxy
    /// (`no-proxy=true`); [`NoProxySetting::List`] = the parsed
    /// reverse-dot-segment-prefix host list.
    pub no_proxy: Option<NoProxySetting>,
}

/// Parsed `no-proxy` value.
///
/// The setting takes either a host-list string or the literal `true`.
/// Per AGENTS.md rule 7 (string-literal-union → `enum`) the two-shape
/// union becomes a closed Rust enum so callers can pattern-match the
/// bypass case without inspecting a sentinel string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoProxySetting {
    /// `no-proxy=true` — bypass every proxy regardless of host.
    Bypass,
    /// Comma-separated host list, trimmed and empties dropped. Match
    /// semantics are reverse-dot-segment-prefix.
    List(Vec<String>),
}

/// Build-time error returned by [`crate::ThrottledClient::for_installs`]
/// when a configured proxy URL is malformed.
///
/// Carries the `ERR_PNPM_INVALID_PROXY` error code so users with a stale
/// `.npmrc` see the same diagnostic when migrating from pnpm to pacquet.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ProxyError {
    #[display("Invalid proxy URL: {url} ({reason})")]
    #[diagnostic(
        code(ERR_PNPM_INVALID_PROXY),
        help(
            "Check the value of `https-proxy`, `http-proxy`, or `proxy` in your .npmrc, or the \
             HTTPS_PROXY / HTTP_PROXY environment variables. URL-encode any special characters \
             in the user:password segment."
        )
    )]
    InvalidProxy { url: String, reason: String },
}

/// Parse a proxy URL, auto-prefixing `http://` when the input lacks an
/// authority so values like `proxy.example:8080` round-trip as a proxy
/// URL. Any other parse failure surfaces as [`ProxyError::InvalidProxy`].
///
/// A successful first parse is only accepted if the URL has a host —
/// Rust's `Url::parse` is permissive enough to accept
/// `proxy.example:8080` as a valid URL with scheme `proxy.example` and
/// path `8080`, which is not what pnpm (or anyone) means by a proxy
/// URL. Requiring `url.host().is_some()` forces such inputs through
/// the `http://`-prefix retry where they parse the way a user expects.
pub(crate) fn parse_proxy_url(raw: &str) -> Result<Url, ProxyError> {
    if let Ok(url) = Url::parse(raw)
        && url.host().is_some()
    {
        return Ok(url);
    }
    Url::parse(&format!("http://{raw}")).ok().filter(|url| url.host().is_some()).ok_or_else(|| {
        ProxyError::InvalidProxy {
            url: raw.to_string(),
            reason: "could not parse as an authority-bearing URL".to_string(),
        }
    })
}

/// Split a proxy URL into its userless form and the
/// (`user`, `password`) pair, percent-decoded. Returns `(url, None)`
/// when the URL has no username.
///
/// The two halves are decoded independently.
pub(crate) fn strip_userinfo(mut url: Url) -> (Url, Option<(String, String)>) {
    let raw_user = url.username();
    if raw_user.is_empty() {
        return (url, None);
    }
    let user = percent_decode_str(raw_user);
    let pass = url.password().map(percent_decode_str).unwrap_or_default();
    // `set_username("")` and `set_password(None)` cannot fail on a URL
    // that already parsed successfully (the scheme is by definition
    // one that supports authority).
    let _ = url.set_username("");
    let _ = url.set_password(None);
    (url, Some((user, pass)))
}

/// Minimal percent-decoder for the user/password halves of a proxy URL
/// userinfo.
///
/// Unlike JavaScript's `decodeURIComponent`, which throws `URIError` on
/// malformed `%XX` sequences (e.g. `%ZZ`), this function intentionally
/// keeps invalid sequences verbatim. The lenient fallback matches what
/// pnpm's interpreter does in practice (a thrown error during proxy
/// setup would surface as `ERR_PNPM_INVALID_PROXY`, but pnpm's flow
/// doesn't validate that strictly either), and is the safer choice in
/// a config path where the alternative is rejecting a half-broken
/// password value.
///
/// Hand-rolled rather than pulling in `percent-encoding` as a direct
/// workspace dep because the only call sites are the two halves of a
/// proxy URL userinfo. The substitution table is exactly the
/// `%XX → byte` form plus pass-through.
pub(crate) fn percent_decode_str(text: &str) -> String {
    let mut out = Vec::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%' && idx + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[idx + 1..idx + 3]).ok();
            if let Some(byte) = hex.and_then(|hex_digits| u8::from_str_radix(hex_digits, 16).ok()) {
                out.push(byte);
                idx += 3;
                continue;
            }
        }
        out.push(bytes[idx]);
        idx += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Pre-built reverse-dot-segment lookup table for the no-proxy bypass.
///
/// Each entry's dot-segments are reversed at construction, and a host
/// matches when the entry's reversed segments are a prefix of the
/// host's reversed segments. `npmjs.org` thus matches
/// `registry.npmjs.org` and `foo.bar.npmjs.org` but not
/// `evilnpmjs.org`. Empty entries (from stray commas) never match.
#[derive(Debug)]
pub(crate) struct NoProxyMatcher {
    bypass: bool,
    entries: Vec<Vec<String>>,
}

impl NoProxyMatcher {
    pub(crate) fn from(setting: Option<&NoProxySetting>) -> Self {
        match setting {
            None => NoProxyMatcher { bypass: false, entries: Vec::new() },
            Some(NoProxySetting::Bypass) => NoProxyMatcher { bypass: true, entries: Vec::new() },
            Some(NoProxySetting::List(list)) => NoProxyMatcher {
                bypass: false,
                entries: list
                    .iter()
                    .map(|entry| entry.split('.').rev().map(str::to_string).collect())
                    .collect(),
            },
        }
    }

    pub(crate) fn matches_host(&self, host: &str) -> bool {
        if self.bypass {
            return true;
        }
        let host_rev: Vec<&str> = host.split('.').rev().collect();
        self.entries.iter().any(|entry_rev| {
            !entry_rev.is_empty()
                && entry_rev.len() <= host_rev.len()
                && entry_rev.iter().zip(host_rev.iter()).all(|(a, b)| a == b)
        })
    }

    pub(crate) fn matches_url(&self, url: &Url) -> bool {
        match url.host_str() {
            Some(host) => self.matches_host(host),
            None => false,
        }
    }
}
