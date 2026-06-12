//! TLS + local-address configuration consumed by
//! [`crate::ThrottledClient::for_installs`].
//!
//! `TlsConfig` holds the resolved `(ca, client_identity_pem, strict_ssl,
//! local_address)` quadruple. Built by `pacquet-config` from the
//! `.npmrc` keys `ca`, `cafile`, `cert`, `key`, `strict-ssl`, and
//! `local-address`. Lives in `pacquet-network` for the same reason
//! [`crate::ProxyConfig`] does — `pacquet-config` depends on
//! `pacquet-network` for `AuthHeaders`, so the inverse direction
//! would form a cycle.
//!
//! Ports the TLS wiring of pnpm v11's
//! [`network/fetch/src/dispatcher.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/network/fetch/src/dispatcher.ts).
//! Parity policy: pnpm performs no PEM parsing in user-space (PEM
//! strings are handed directly to Node `tls` / undici, which parse
//! internally), emits no `ERR_PNPM_*` codes for malformed TLS
//! material, silently ignores a missing `cafile`, and consults no
//! environment variables. Pacquet mirrors each of those choices.

use crate::auth::nerf_dart;
use std::{collections::HashMap, net::IpAddr};

/// Resolved TLS + local-address configuration.
///
/// All fields are optional. `strict_ssl` is `None` here because pnpm
/// applies the `true` default at every read site
/// ([`dispatcher.ts:191,197,241,295`](https://github.com/pnpm/pnpm/blob/94240bc046/network/fetch/src/dispatcher.ts#L191))
/// rather than baking it into the config layer — pacquet does the
/// same so a user that explicitly sets `strict-ssl=false` stays
/// distinguishable from "unset". The default value is applied at
/// client-build time by [`crate::ThrottledClient::for_installs`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TlsConfig {
    /// CA certificate chain to trust for TLS verification. Each
    /// element is a PEM-encoded certificate. Populated by `.npmrc`'s
    /// `ca` key (inline PEM, possibly multiple via array shape) or by
    /// reading `cafile` (which gets split on
    /// `-----END CERTIFICATE-----` to mirror pnpm's
    /// [loader behavior](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/loadNpmrcFiles.ts#L249-L255)).
    /// `cafile`-not-found is silently treated as unset, matching
    /// upstream.
    pub ca: Vec<String>,

    /// PEM-encoded client certificate, when client-cert auth is
    /// required by the registry. Set from `.npmrc`'s `cert` key.
    pub cert: Option<String>,

    /// PEM-encoded client private key. Paired with [`Self::cert`] when
    /// both are set — concatenated and handed to reqwest's
    /// `Identity::from_pem` (rustls single-buffer form). Accepts
    /// PKCS#1, PKCS#8, and EC PEM keys (the same surface Node's
    /// `tls` exposes to pnpm). Set from `.npmrc`'s `key` key.
    pub key: Option<String>,

    /// `strict-ssl` toggle. `None` = unset (defaults to `true` at
    /// apply time); `Some(true)` = explicit strict (same as default);
    /// `Some(false)` = disable both cert-chain and hostname
    /// verification (matches Node's `rejectUnauthorized=false` which
    /// short-circuits SNI / hostname checks too). Maps to reqwest's
    /// `ClientBuilder::danger_accept_invalid_certs`.
    pub strict_ssl: Option<bool>,

    /// Outbound interface IP. Maps to reqwest's
    /// `ClientBuilder::local_address`. pnpm passes the value as a
    /// bare string with no validation. Pacquet parses it as
    /// [`IpAddr`] in the config layer and silently drops anything
    /// that doesn't parse — mirroring pnpm's parity policy of letting
    /// the network layer surface the failure when (and if) the value
    /// actually gets used at connect time. A future enhancement could
    /// emit a warning at parse time; tracked alongside the rest of
    /// the TLS error-surface work.
    pub local_address: Option<IpAddr>,
}

/// Build-time error returned by [`crate::ThrottledClient::for_installs`]
/// when configured TLS material is invalid.
///
/// pnpm does not define `ERR_PNPM_INVALID_CA` / `ERR_PNPM_INVALID_CERT`
/// / `ERR_PNPM_INVALID_KEY` error codes — invalid PEM surfaces as raw
/// `tls.connect` errors at request time
/// ([`dispatcher.ts:184-200`](https://github.com/pnpm/pnpm/blob/94240bc046/network/fetch/src/dispatcher.ts#L184-L200)).
/// Pacquet validates eagerly because reqwest's `Certificate::from_pem`
/// / `Identity::from_pem` return errors up-front and pushing that to
/// per-request time would silently degrade every install behind a
/// broken `ca`. Diagnostic messages are plain prose; no code
/// attribute is emitted so reviewers can see at a glance that this is
/// a pacquet-only diagnostic, not a pnpm error code.
#[derive(Debug, derive_more::Display, derive_more::Error, miette::Diagnostic)]
#[non_exhaustive]
pub enum TlsError {
    /// `Certificate::from_pem` rejected one of the `ca` entries.
    /// `index` is the 0-based position within the resolved CA list.
    #[display("Invalid CA certificate (entry {index}): {reason}")]
    InvalidCa { index: usize, reason: String },

    /// `Identity::from_pem` rejected the concatenated `cert` +
    /// `key` PEM pair. Rustls accepts PKCS#1, PKCS#8, and EC keys —
    /// landing here means the bytes aren't a valid PEM in any of
    /// those formats (corrupt, base64-mangled, missing the
    /// `-----BEGIN…-----` armor, etc.).
    #[display("Invalid client TLS cert/key: {reason}")]
    InvalidClientIdentity { reason: String },
}

/// Per-registry TLS overrides keyed by nerf-darted registry URI.
///
/// Mirrors pnpm v11's
/// [`configByUri[<uri>].tls`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/getNetworkConfigs.ts#L34-L40)
/// shape: each entry is the `(ca, cert, key)` triple a request to that
/// registry should use *instead of* the corresponding top-level fields
/// — matching upstream's `{ ...opts, ...sslConfig }` spread at
/// [`dispatcher.ts:143,264`](https://github.com/pnpm/pnpm/blob/94240bc046/network/fetch/src/dispatcher.ts#L143)
/// where per-registry values override top-level **field-by-field**.
///
/// Lookup is via [`PerRegistryTls::pick_for_url`] with the 5-step
/// fallback chain pnpm uses (exact > nerf-dart > no-port > shorter
/// prefix > recursive no-port retry).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PerRegistryTls {
    /// Keys are nerf-darted URIs (`//host[:port]/path/` form). Values
    /// hold the explicit overrides for that prefix — every field is
    /// `Option` because pnpm allows partial overrides (e.g. only `ca`
    /// scoped, with `cert` / `key` falling through to top-level).
    by_uri: HashMap<String, RegistryTls>,
    /// Cache of `key.split('/').count()` maxed across `by_uri.keys()`.
    /// Bounds the path-prefix walk in [`Self::pick_for_url`] so the
    /// loop stops after the longest user-supplied prefix instead of
    /// down to `//`.
    max_parts: usize,
}

/// `(ca, cert, key)` triple for a single registry override. Each field
/// is post-`\n`-expansion / post-file-read PEM string — the parser in
/// `pacquet-config::npmrc_auth` normalizes both shapes (`:ca=` inline
/// and `:cafile=<path>` file-read) into the same `Option<String>` slot
/// so the network layer sees one form.
///
/// Why `Option<String>` for `ca` (not `Vec<String>` like
/// [`TlsConfig::ca`]): pnpm's per-registry parser stores a single
/// string per `(uri, field)` slot — multi-cert bundles arrive as one
/// PEM string with embedded `-----END CERTIFICATE-----` delimiters,
/// which `reqwest::Certificate::from_pem` accepts. See
/// [`getNetworkConfigs.ts:37`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/getNetworkConfigs.ts#L37)
/// for the upstream shape.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RegistryTls {
    /// Per-registry CA override. May contain multiple
    /// `-----END CERTIFICATE-----`-delimited PEMs in one string when
    /// sourced from `:cafile=<path>`, or a single PEM (with `\n`
    /// escapes expanded) when sourced from `:ca=...`.
    pub ca: Option<String>,
    /// Per-registry client certificate PEM.
    pub cert: Option<String>,
    /// Per-registry client private key PEM. Accepts PKCS#1, PKCS#8,
    /// and EC keys (handed to reqwest's `Identity::from_pem` on the
    /// rustls backend — see the comment on `apply_tls` in
    /// `crates/network/src/lib.rs`).
    pub key: Option<String>,
}

impl RegistryTls {
    /// `true` when no field is set. Used by callers that want to skip
    /// building a per-registry client for an empty override.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ca.is_none() && self.cert.is_none() && self.key.is_none()
    }
}

impl PerRegistryTls {
    /// Build from a nerf-darted → [`RegistryTls`] map. Drops empty
    /// entries (matches pnpm — an empty `tls` object is the same as
    /// no entry at all).
    #[must_use]
    pub fn from_map(by_uri: HashMap<String, RegistryTls>) -> Self {
        let by_uri: HashMap<_, _> = by_uri.into_iter().filter(|(_, v)| !v.is_empty()).collect();
        let max_parts = by_uri.keys().map(|key| key.split('/').count()).max().unwrap_or(0);
        PerRegistryTls { by_uri, max_parts }
    }

    /// `true` when there are no per-registry overrides. Lets the
    /// network layer skip the per-registry-client construction path.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_uri.is_empty()
    }

    /// Iterate `(nerf_dart_uri, &RegistryTls)` pairs. The network
    /// layer uses this to pre-build a client per unique override
    /// combo.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &RegistryTls)> {
        self.by_uri.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Look up the per-registry override for `url` via pnpm's
    /// [`pickSettingByUrl`](https://github.com/pnpm/pnpm/blob/94240bc046/network/fetch/src/dispatcher.ts#L338-L375)
    /// 5-step fallback chain:
    ///
    /// 1. Exact URL match.
    /// 2. Nerf-darted URL.
    /// 3. URL without port (recurse).
    /// 4. Progressively shorter nerf-darted path prefixes.
    /// 5. Retry recursively without port.
    ///
    /// Returns the **nerf-darted key** that matched (so the network
    /// layer can index into its pre-built per-registry client map),
    /// not the `RegistryTls` itself.
    #[must_use]
    pub fn pick_for_url(&self, url: &str) -> Option<&str> {
        if self.by_uri.is_empty() {
            return None;
        }
        // Step 1: exact URL.
        if let Some((key, _)) = self.by_uri.get_key_value(url) {
            return Some(key.as_str());
        }
        // Step 2: nerf-darted URL.
        let nerf = nerf_dart(url);
        if !nerf.is_empty()
            && let Some((key, _)) = self.by_uri.get_key_value(nerf.as_str())
        {
            return Some(key.as_str());
        }
        // Step 4: walk progressively shorter prefixes of the
        // nerf-darted form. `nerf` is `//host[:port]/path/`, splitting
        // on `/` yields `["", "", "host[:port]", "path", "", ""]` or
        // similar; the loop iterates from the longest meaningful
        // prefix down to `//host[:port]/`.
        if !nerf.is_empty() {
            let parts: Vec<&str> = nerf.split('/').collect();
            let upper = parts.len().min(self.max_parts);
            for i in (3..upper).rev() {
                let key = format!("{}/", parts[..i].join("/"));
                if let Some((found, _)) = self.by_uri.get_key_value(key.as_str()) {
                    return Some(found.as_str());
                }
            }
        }
        // Steps 3 + 5: strip any port from the URL and retry. We do
        // this *after* the nerf-dart walk because the walk already
        // handles host-prefix shortening; the port-strip only matters
        // when the user keyed `//host/` (no port) but the request URL
        // carries an explicit port.
        let stripped = strip_port(url);
        if stripped != url {
            return self.pick_for_url(&stripped);
        }
        None
    }

    /// Borrow the inner [`RegistryTls`] for a nerf-darted key. Returns
    /// `None` when the key wasn't registered.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&RegistryTls> {
        self.by_uri.get(key)
    }
}

/// Strip an explicit `:port` from an `http(s)://host[:port]/path...`
/// URL, returning the modified URL string with a trailing `/` when
/// the input had one. Returns the original string when there's no
/// port to strip.
///
/// Hand-rolled instead of pulling in the `url` crate as a direct dep
/// (mirrors the `ParsedUrl` approach for the auth nerf-darting). The
/// contract: only HTTP-family URLs flow through here, ports are
/// always numeric, and authority is the `[user@]host[:port]` segment
/// before the first `/`.
fn strip_port(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let (authority, path_tail) = match rest.split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (rest, None),
    };
    // Skip past any `user[:pw]@` userinfo. The port-bearing colon is
    // the one in the host segment, not in the userinfo.
    let host_segment = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let userinfo = authority.strip_suffix(host_segment).unwrap_or("");
    // IPv6 literals like `[::1]:8080` have `:` inside the brackets;
    // find the port colon only *after* a closing `]` when present.
    let port_colon = if let Some(bracket_end) = host_segment.find(']') {
        host_segment[bracket_end..].find(':').map(|offset| bracket_end + offset)
    } else {
        host_segment.find(':')
    };
    let Some(idx) = port_colon else {
        return url.to_string();
    };
    let host_no_port = &host_segment[..idx];
    match path_tail {
        Some(path) => format!("{scheme}://{userinfo}{host_no_port}/{path}"),
        None => format!("{scheme}://{userinfo}{host_no_port}/"),
    }
}

#[cfg(test)]
mod tests;
