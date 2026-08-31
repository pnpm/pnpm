use std::{fmt, time::Duration};

use indexmap::IndexMap;
use pnpm_env_replace::EnvVar;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;

use pnpr_error::RegistryError;
use pnpr_policy::{AccessList, PackageRules};

use super::{AccessSpec, Teams};

/// Runtime upstream declaration: the upstream `url`, the request headers
/// pnpr attaches to every fetch it makes to that upstream, and the
/// verdaccio per-upstream tuning knobs (`maxage`, `timeout`, `max_fails`,
/// `fail_timeout`, `cache`).
///
/// [`Self::headers`] is resolved once, at config load, from the YAML
/// `auth:` block (an `Authorization` header derived from
/// `type`/`token`/`token_env`) merged with the `headers:` map. The
/// parse-time shape lives in `UpstreamConfigFile`; `resolve_upstream_config` turns
/// one into the other. Verdaccio fields pnpr doesn't model yet
/// (agent options, `strict_ssl`, ...) are accepted and dropped.
#[derive(Clone)]
pub struct UpstreamConfig {
    pub url: String,
    /// Auth + custom headers, fully resolved and ready to attach to
    /// every request pnpr makes to this upstream.
    pub headers: HeaderMap,
    /// Per-upstream packument freshness window (verdaccio's `maxage`).
    /// `None` when the YAML omits it — the proxy then falls back to the
    /// global [`super::Config::packument_ttl`], so the existing
    /// `--packument-ttl-secs` flag still governs upstreams that don't set
    /// their own.
    pub maxage: Option<Duration>,
    /// Per-request deadline for every fetch to this upstream (verdaccio's
    /// `timeout`). Defaults to [`Self::DEFAULT_TIMEOUT`].
    pub timeout: Duration,
    /// Consecutive failures before the upstream is treated as down
    /// (verdaccio's `max_fails`). Defaults to [`Self::DEFAULT_MAX_FAILS`].
    pub max_fails: u32,
    /// How long a down upstream stays down before pnpr retries it
    /// (verdaccio's `fail_timeout`). Defaults to
    /// [`Self::DEFAULT_FAIL_TIMEOUT`].
    pub fail_timeout: Duration,
    /// Whether tarballs fetched from this upstream are written to the local
    /// mirror (verdaccio's `cache`). `false` streams them through
    /// uncached. Defaults to `true`.
    pub cache: bool,
    /// Which pnpr callers may select this upstream as a proxied private-route
    /// credential, and reach it through its `/~<name>/` registry endpoint.
    /// `None` means the upstream is registry-proxy only and is never offered as
    /// a resolver private-route credential — only upstreams that declare
    /// `access:` participate in route classification.
    pub access: Option<AccessList>,
    /// The registry's `packages:` map: the namespace it claims plus
    /// per-package `access` refinements (a `publish`/`unpublish` value is a
    /// config error — no write can land on an upstream). The registry-level
    /// gate ([`Self::access`], or `$all` for a public upstream) is the default
    /// an entry's omitted `access` falls back to.
    pub rules: PackageRules,
}

impl UpstreamConfig {
    /// Verdaccio's `timeout` default (`30s`).
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
    /// Verdaccio's `max_fails` default (`2`).
    pub const DEFAULT_MAX_FAILS: u32 = 2;
    /// Verdaccio's `fail_timeout` default (`5m`).
    pub const DEFAULT_FAIL_TIMEOUT: Duration = Duration::from_mins(5);

    /// Build a bare upstream with just a URL and headers, all tuning knobs
    /// at their verdaccio defaults. Used by the programmatic
    /// [`super::Config::proxy`] constructor and tests.
    #[must_use]
    pub fn with_defaults(url: String, headers: HeaderMap) -> Self {
        Self {
            url,
            headers,
            maxage: None,
            timeout: Self::DEFAULT_TIMEOUT,
            max_fails: Self::DEFAULT_MAX_FAILS,
            fail_timeout: Self::DEFAULT_FAIL_TIMEOUT,
            cache: true,
            access: None,
            rules: PackageRules::default(),
        }
    }
}

impl fmt::Debug for UpstreamConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpstreamConfig")
            .field("url", &self.url)
            .field("headers", &RedactedHeaders(&self.headers))
            .field("maxage", &self.maxage)
            .field("timeout", &self.timeout)
            .field("max_fails", &self.max_fails)
            .field("fail_timeout", &self.fail_timeout)
            .field("cache", &self.cache)
            .field("access", &self.access)
            .field("rules", &self.rules)
            .finish()
    }
}

/// Wraps a [`HeaderMap`] so its `Debug` lists header names with values
/// redacted. Upstream headers carry credentials (an `Authorization`, or
/// an API key in a custom header), and those must never reach a log
/// line, span, or diagnostic dump.
pub struct RedactedHeaders<'a>(pub &'a HeaderMap);

impl fmt::Debug for RedactedHeaders<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.0.keys().map(|name| (name.as_str(), "<redacted>"))).finish()
    }
}

/// The serving knobs of an upstream registry, in verdaccio's upstream shape for
/// the subset pnpr implements: `url`, an `auth:` block, and a free-form
/// `headers:` map. Built from an `upstream:` registry entry
/// ([`super::resolve_upstream_registry`]) and resolved into [`UpstreamConfig`] by
/// [`resolve_upstream_config`].
#[derive(Debug, Deserialize)]
pub(super) struct UpstreamConfigFile {
    pub(super) url: String,
    #[serde(default)]
    pub(super) auth: Option<UpstreamAuthFile>,
    #[serde(default)]
    pub(super) headers: IndexMap<String, String>,
    /// Verdaccio interval strings (`"2m"`, `"30s"`, `"1h30m"`) or a bare
    /// number of seconds; parsed by [`parse_interval`] in
    /// [`resolve_upstream_config`]. Kept as raw strings here so an unparsable
    /// value surfaces as a config error rather than a serde failure.
    #[serde(default)]
    pub(super) maxage: Option<Interval>,
    #[serde(default)]
    pub(super) timeout: Option<Interval>,
    #[serde(default)]
    pub(super) max_fails: Option<u32>,
    #[serde(default)]
    pub(super) fail_timeout: Option<Interval>,
    #[serde(default)]
    pub(super) cache: Option<bool>,
    /// Which pnpr callers may select this upstream as a proxied private-route
    /// credential. Its presence is what promotes a plain proxy upstream into a
    /// resolver private-route credential exposed at `/~<name>/`.
    #[serde(default)]
    pub(super) access: Option<AccessSpec>,
}

/// A verdaccio interval scalar as written in YAML: either a string
/// (`"2m"`, `"30s"`) or a bare number (a count of seconds). Both YAML
/// shapes are accepted — verdaccio configs use either — and kept as the
/// raw string so [`parse_interval`] handles them uniformly and an
/// unparsable value surfaces as a precise config error.
#[derive(Debug, Clone)]
pub(super) struct Interval(pub(super) String);

impl<'de> Deserialize<'de> for Interval {
    fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
    where
        De: serde::Deserializer<'de>,
    {
        struct IntervalVisitor;
        impl serde::de::Visitor<'_> for IntervalVisitor {
            type Value = Interval;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(r#"an interval string like "2m" or a number of seconds"#)
            }
            fn visit_str<DeError>(self, value: &str) -> Result<Interval, DeError> {
                Ok(Interval(value.to_string()))
            }
            fn visit_i64<DeError>(self, value: i64) -> Result<Interval, DeError> {
                Ok(Interval(value.to_string()))
            }
            fn visit_u64<DeError>(self, value: u64) -> Result<Interval, DeError> {
                Ok(Interval(value.to_string()))
            }
            fn visit_f64<DeError>(self, value: f64) -> Result<Interval, DeError> {
                Ok(Interval(value.to_string()))
            }
        }
        deserializer.deserialize_any(IntervalVisitor)
    }
}

/// The YAML `auth:` block on an upstream. `token` takes priority over
/// `token_env`; either resolves to the credential placed in the
/// `Authorization` header, encoded per [`UpstreamAuthType`].
#[derive(Debug, Deserialize)]
pub(super) struct UpstreamAuthFile {
    pub(super) r#type: UpstreamAuthType,
    #[serde(default)]
    pub(super) token: Option<String>,
    #[serde(default)]
    pub(super) token_env: Option<TokenEnv>,
}

/// How the resolved token is encoded into the `Authorization` header:
/// `bearer` → `Bearer <token>`, `basic` → `Basic <token>` (the token
/// is used verbatim, matching verdaccio's assumption that a `basic`
/// token is already a base64 `user:pass`).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum UpstreamAuthType {
    Bearer,
    Basic,
}

/// Verdaccio's `token_env`: either the boolean `true` (read the
/// default `NPM_TOKEN` env var) or a string naming the env var to
/// read. `false` reads nothing.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum TokenEnv {
    Flag(bool),
    Named(String),
}

impl TokenEnv {
    /// Default env var name verdaccio reads for `token_env: true`.
    const DEFAULT_VAR: &'static str = "NPM_TOKEN";

    /// The env var name to read, or `None` for `token_env: false`.
    fn var_name(&self) -> Option<&str> {
        match self {
            TokenEnv::Flag(true) => Some(Self::DEFAULT_VAR),
            TokenEnv::Flag(false) => None,
            TokenEnv::Named(name) => Some(name),
        }
    }
}

/// Resolve one parsed [`UpstreamConfigFile`] into a runtime [`UpstreamConfig`],
/// baking the `auth:` credential and `headers:` map into a single
/// [`HeaderMap`]. Reads env vars (for `token_env`) through `Sys` so
/// the resolution is testable.
///
/// The auth-derived `Authorization` header is inserted first, then the
/// custom `headers:` are merged on top — so a custom `Authorization`
/// entry overrides the one derived from `auth:`, matching verdaccio's
/// merge order. A configured `auth:` block that resolves to no token,
/// an unknown header name, or a non-ASCII header value is a config
/// error rather than a silent unauthenticated request.
pub(super) fn resolve_upstream_config<Sys: EnvVar>(
    name: &str,
    file: UpstreamConfigFile,
    teams: &Teams,
) -> Result<UpstreamConfig, RegistryError> {
    let mut headers = HeaderMap::new();
    if let Some(auth) = &file.auth {
        let token =
            resolve_upstream_token::<Sys>(auth).ok_or_else(|| RegistryError::InvalidConfig {
                reason: format!(
                    "upstream {name:?} has an auth block but no token could be resolved \
                     (set auth.token or point auth.token_env at a set env var)",
                ),
            })?;
        let value = match auth.r#type {
            UpstreamAuthType::Bearer => format!("Bearer {token}"),
            UpstreamAuthType::Basic => format!("Basic {token}"),
        };
        let value = HeaderValue::from_str(&value).map_err(|_| RegistryError::InvalidConfig {
            reason: format!("upstream {name:?} auth token is not a valid header value"),
        })?;
        headers.insert(AUTHORIZATION, value);
    }
    for (raw_name, raw_value) in &file.headers {
        let header_name = HeaderName::from_bytes(raw_name.as_bytes()).map_err(|_| {
            RegistryError::InvalidConfig {
                reason: format!("upstream {name:?} has an invalid header name {raw_name:?}"),
            }
        })?;
        let header_value =
            HeaderValue::from_str(raw_value).map_err(|_| RegistryError::InvalidConfig {
                reason: format!("upstream {name:?} header {raw_name:?} has an invalid value"),
            })?;
        headers.insert(header_name, header_value);
    }

    // Parse the verdaccio interval knobs, turning a typo'd value into a
    // config error (named for the offending field) rather than silently
    // falling back to the default.
    let parse_field = |field: &str,
                       raw: &Option<Interval>|
     -> Result<Option<Duration>, RegistryError> {
        raw.as_ref()
            .map(|Interval(value)| {
                parse_interval(value).ok_or_else(|| RegistryError::InvalidConfig {
                    reason: format!("upstream {name:?} has an invalid {field} interval {value:?}"),
                })
            })
            .transpose()
    };
    let maxage = parse_field("maxage", &file.maxage)?;
    let timeout = parse_field("timeout", &file.timeout)?.unwrap_or(UpstreamConfig::DEFAULT_TIMEOUT);
    let fail_timeout = parse_field("fail_timeout", &file.fail_timeout)?
        .unwrap_or(UpstreamConfig::DEFAULT_FAIL_TIMEOUT);
    let access = file.access.as_ref().map(|spec| spec.to_access_list(teams)).transpose().map_err(
        |reason| RegistryError::InvalidConfig {
            reason: format!("upstream {name:?} has an invalid `access` list: {reason}"),
        },
    )?;

    Ok(UpstreamConfig {
        url: file.url,
        headers,
        maxage,
        timeout,
        max_fails: file.max_fails.unwrap_or(UpstreamConfig::DEFAULT_MAX_FAILS),
        fail_timeout,
        cache: file.cache.unwrap_or(true),
        access,
        // The `packages:` rules are attached by the caller
        // (`build_registries`) — this resolver only handles the serving
        // knobs shared with programmatic construction.
        rules: PackageRules::default(),
    })
}

/// Parse a verdaccio-style interval string into a [`Duration`].
///
/// Accepts the suffixes verdaccio's `parseInterval` understands —
/// `ms`, `s`, `m`, `h`, `d`, `w` — optionally chained without or with
/// whitespace (`"1h30m"`, `"2m 30s"`), and a bare number, which (like
/// verdaccio) is read as **seconds**. A trailing number with no suffix
/// is also seconds. Returns `None` for anything unparsable so the
/// caller can surface a precise config error.
pub(super) fn parse_interval(raw: &str) -> Option<Duration> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // A bare number is seconds, matching verdaccio (`interval * 1000` ms).
    if let Ok(seconds) = raw.parse::<f64>() {
        // `try_from_secs_f64` rejects negative, non-finite, and
        // out-of-range values, so an absurd config (`"1e30"`) surfaces as
        // a config error rather than panicking pnpr at startup.
        return Duration::try_from_secs_f64(seconds).ok();
    }
    let mut total_seconds = 0f64;
    let bytes = raw.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        let number_start = index;
        while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
            index += 1;
        }
        if index == number_start {
            return None;
        }
        let number: f64 = raw[number_start..index].parse().ok()?;
        let unit_start = index;
        while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
            index += 1;
        }
        let seconds = match &raw[unit_start..index] {
            "ms" => number / 1000.0,
            "s" | "" => number,
            "m" => number * 60.0,
            "h" => number * 3600.0,
            "d" => number * 86_400.0,
            "w" => number * 604_800.0,
            _ => return None,
        };
        total_seconds += seconds;
    }
    // Fallible conversion so an overflowing compound (`"999999999999w"`)
    // is rejected as unparsable rather than panicking.
    Duration::try_from_secs_f64(total_seconds).ok()
}

/// Pick the credential for an upstream's `auth:` block: an explicit
/// `token` wins; otherwise read the env var named by `token_env`.
fn resolve_upstream_token<Sys: EnvVar>(auth: &UpstreamAuthFile) -> Option<String> {
    if let Some(token) = &auth.token {
        return non_empty_token(token);
    }
    let var_name = auth.token_env.as_ref()?.var_name()?;
    Sys::var(var_name).and_then(|token| non_empty_token(&token))
}

fn non_empty_token(token: &str) -> Option<String> {
    (!token.trim().is_empty()).then(|| token.to_string())
}
