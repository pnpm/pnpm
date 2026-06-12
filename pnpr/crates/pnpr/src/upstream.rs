use crate::{
    config::RedactedHeaders,
    error::{RegistryError, Result},
    package_name::PackageName,
};
use chrono::{DateTime, Duration, Timelike, Utc};
use pacquet_network::ThrottledClient;
use reqwest::{
    StatusCode,
    header::{self, HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fmt, sync::Arc};

/// Wraps a shared [`ThrottledClient`] (so the registry inherits pnpm's
/// tuned reqwest defaults: `User-Agent: pnpm`, HTTP/1.1, hickory DNS,
/// pool/timeout tuning, concurrency semaphore, and per-registry TLS
/// routing if it's ever wired in later) and adds the small bit of
/// glue specific to a proxy: building the upstream URL and fishing
/// the packument or tarball response out of it.
#[derive(Clone)]
pub struct Upstream {
    client: Arc<ThrottledClient>,
    base: String,
    /// Resolved per-uplink request headers (auth + custom) attached to
    /// every fetch. Empty for an uplink with no `auth:`/`headers:`.
    headers: HeaderMap,
}

impl fmt::Debug for Upstream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Upstream")
            .field("client", &self.client)
            .field("base", &self.base)
            .field("headers", &RedactedHeaders(&self.headers))
            .finish()
    }
}

#[derive(Debug)]
pub enum FetchOutcome<Payload> {
    /// Upstream returned content.
    Ok(Payload),
    /// Upstream returned 404. The caller should propagate this verbatim.
    NotFound,
}

/// Conditional-GET validators captured from an upstream packument
/// response (its `ETag` / `Last-Modified`) and replayed on the next
/// refresh as `If-None-Match` / `If-Modified-Since`. An upstream that
/// emits neither leaves both `None`, and the refresh falls back to an
/// unconditional GET.
///
/// Persisted verbatim in a sidecar next to the cached packument (see
/// [`crate::storage`]); the field names double as the on-disk JSON keys.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CacheValidators {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub etag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_modified: Option<String>,
}

impl CacheValidators {
    pub fn is_empty(&self) -> bool {
        self.etag.is_none() && self.last_modified.is_none()
    }

    fn from_headers(headers: &HeaderMap) -> Self {
        let get =
            |name| headers.get(name).and_then(|value| value.to_str().ok()).map(str::to_string);
        Self { etag: get(header::ETAG), last_modified: get(header::LAST_MODIFIED) }
    }
}

/// A packument fetched (or revalidated) against an upstream.
#[derive(Debug)]
pub struct FetchedPackument {
    pub bytes: Vec<u8>,
    pub validators: CacheValidators,
}

/// Outcome of a (possibly conditional) packument fetch.
#[derive(Debug)]
pub enum PackumentFetch {
    /// Upstream returned a fresh body, along with its cache validators.
    Modified(FetchedPackument),
    /// Upstream answered `304 Not Modified`: the validators we sent are
    /// still current, so the caller should keep serving its cached copy.
    NotModified,
    /// Upstream returned 404.
    NotFound,
}

impl Upstream {
    pub fn new(base: String, headers: HeaderMap) -> Self {
        Self { client: Arc::new(ThrottledClient::new_for_installs()), base, headers }
    }

    /// Fetch a package's packument, conditionally when `validators`
    /// carries an `ETag`/`Last-Modified`. A `304 Not Modified` short-
    /// circuits to [`PackumentFetch::NotModified`] without a body, so the
    /// caller can keep serving its cached copy — the bandwidth win on a
    /// stale-but-current packument.
    pub async fn fetch_packument(
        &self,
        name: &PackageName,
        validators: &CacheValidators,
    ) -> Result<PackumentFetch> {
        let url = format!("{}/{}", self.base.trim_end_matches('/'), name.as_str());
        let client = self.client.acquire_for_url(&url).await;
        let mut request = client.get(&url).headers(self.headers.clone());
        let mut sent_conditional = false;
        if let Some(etag) = &validators.etag
            && let Ok(value) = HeaderValue::from_str(etag)
        {
            request = request.header(header::IF_NONE_MATCH, value);
            sent_conditional = true;
        }
        if let Some(last_modified) = &validators.last_modified
            && let Ok(value) = HeaderValue::from_str(last_modified)
        {
            request = request.header(header::IF_MODIFIED_SINCE, value);
            sent_conditional = true;
        }
        let response = request
            .send()
            .await
            .map_err(|source| RegistryError::Upstream { url: url.clone(), source })?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(PackumentFetch::NotFound);
        }
        // Only honor a `304` if we actually sent a conditional header. A
        // `304` to an unconditional request is a misbehaving upstream and
        // carries no body to serve, so let it fall through to
        // `check_status` and surface as an upstream status error rather
        // than reusing a (possibly nonexistent) cached copy.
        if sent_conditional && response.status() == StatusCode::NOT_MODIFIED {
            return Ok(PackumentFetch::NotModified);
        }
        let response = check_status(response, &url).await?;
        let validators = CacheValidators::from_headers(response.headers());
        let bytes = response
            .bytes()
            .await
            .map_err(|source| RegistryError::Upstream { url: url.clone(), source })?;
        Ok(PackumentFetch::Modified(FetchedPackument { bytes: bytes.to_vec(), validators }))
    }

    /// Send the tarball request and return the streaming
    /// [`reqwest::Response`] so the caller can pipe the body straight
    /// to the client without buffering. Status and 404 handling
    /// happen here before any bytes are forwarded.
    pub async fn fetch_tarball_response(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<FetchOutcome<reqwest::Response>> {
        let url = format!("{}/{}/-/{}", self.base.trim_end_matches('/'), name.as_str(), filename);
        let client = self.client.acquire_for_url(&url).await;
        let response = client
            .get(&url)
            .headers(self.headers.clone())
            .send()
            .await
            .map_err(|source| RegistryError::Upstream { url: url.clone(), source })?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(FetchOutcome::NotFound);
        }
        let response = check_status(response, &url).await?;
        Ok(FetchOutcome::Ok(response))
    }
}

async fn check_status(response: reqwest::Response, url: &str) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(RegistryError::UpstreamStatus { url: url.to_string(), status: status.as_u16(), body })
}

/// Rewrite every `dist.tarball` in `value` to a URL served by *this*
/// registry instead of whatever URL the source put there. The new
/// URL is `{public_url}/{pkg}/-/{basename}`, where `basename` is the
/// last `/`-separated segment of the original tarball URL. This
/// handles both npm's canonical `/{pkg}/-/{basename}` shape and
/// verdaccio's `/{scope}/{name}/-/{scope}/{filename}` shape uniformly
/// — we only look at the basename, never at the path prefix.
///
/// Walks both packument shape (`{ "versions": { v: { dist: ... } } }`)
/// and single-version manifest shape (`{ dist: ... }` at the top
/// level) so a single helper covers both endpoints.
pub fn rewrite_tarball_urls(value: &mut Value, pkg: &PackageName, public_url: &str) {
    let public_url = public_url.trim_end_matches('/');
    if let Some(versions) = value.get_mut("versions").and_then(Value::as_object_mut) {
        for version in versions.values_mut() {
            rewrite_dist_tarball(version, pkg, public_url);
        }
    }
    rewrite_dist_tarball(value, pkg, public_url);
}

fn rewrite_dist_tarball(value: &mut Value, pkg: &PackageName, public_url: &str) {
    let Some(dist) = value.get_mut("dist").and_then(Value::as_object_mut) else {
        return;
    };
    let Some(tarball_value) = dist.get_mut("tarball") else { return };
    let Some(basename) = tarball_value.as_str().and_then(|url| url.rsplit('/').next()) else {
        return;
    };
    *tarball_value = Value::String(format!("{public_url}/{}/-/{basename}", pkg.as_str()));
}

/// Look up the version manifest for `version_or_tag` inside a parsed
/// packument: if the string matches a dist-tag it resolves through
/// `dist-tags[tag]` first, otherwise it's taken as a literal version.
/// Returns the version's manifest *with* the `dist.tarball` rewritten
/// to point at this server.
pub fn extract_version_manifest(
    packument: &Value,
    pkg: &PackageName,
    version_or_tag: &str,
    public_url: &str,
) -> Option<Value> {
    let resolved = packument
        .get("dist-tags")
        .and_then(|tags| tags.get(version_or_tag))
        .and_then(Value::as_str)
        .unwrap_or(version_or_tag);
    let mut manifest = packument.get("versions")?.get(resolved)?.clone();
    rewrite_tarball_urls(&mut manifest, pkg, public_url);
    Some(manifest)
}

/// Top-level packument fields *copied verbatim* into the abbreviated
/// (`application/vnd.npm.install-v1+json`) form.
///
/// `time` isn't here because it's coarsened rather than copied — see
/// [`coarsen_time_map`]. It goes beyond the npm spec but the
/// pnpm/pacquet resolvers read it for the `minimumReleaseAge` check,
/// so it stays (in a shrunken form).
///
/// `modified` isn't here either because it's synthesized: it's
/// extracted from `time.modified` (real npm packuments nest it
/// there). pacquet reads `meta.modified` in its version-pick
/// heuristics (`pick_package_from_meta.rs`) and as a freshness check
/// (`pick_package.rs`); omitting it pushes the resolver onto a slower
/// fallback path.
const ABBREVIATED_TOP_FIELDS: &[&str] = &["name", "dist-tags"];

/// Age past which a `time` entry loses its time-of-day in the
/// abbreviated form, keeping only the (rounded-up) bare date.
/// `minimumReleaseAge` cutoffs sit in the recent past (days, not
/// weeks), so a week-old entry rounded up to the next day is still
/// unambiguously on the mature side of any realistic cutoff. See
/// [`coarsen_time_map`].
const TIME_PRECISION_HORIZON_DAYS: i64 = 7;

/// Per-version fields preserved in the abbreviated form — a subset of
/// the npm spec's abbreviated version object
/// (<https://github.com/npm/registry/blob/master/docs/responses/package-metadata.md#abbreviated-version-object>).
/// Fields neither the pnpm nor the pacquet resolver reads are dropped
/// to shrink the document: `funding`, `acceptDependencies`,
/// `_hasShrinkwrap`, and `devDependencies` (a dependency's dev
/// dependencies are never installed). Redundant `dist` subfields are
/// trimmed per-version by [`trim_dist_fields`].
const ABBREVIATED_VERSION_FIELDS: &[&str] = &[
    "name",
    "version",
    "deprecated",
    "bin",
    "dist",
    "engines",
    "directories",
    "dependencies",
    "peerDependencies",
    "optionalDependencies",
    "bundleDependencies",
    "cpu",
    "os",
    "libc",
    "peerDependenciesMeta",
    "hasInstallScript",
];

/// Strip a parsed packument down to the abbreviated install-v1 form.
/// Should be called *after* `rewrite_tarball_urls` so the returned
/// document's `dist.tarball` URLs already point at this server.
pub fn abbreviate_packument(packument: &Value, now: DateTime<Utc>) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(obj) = packument.as_object() {
        for &field in ABBREVIATED_TOP_FIELDS {
            if let Some(value) = obj.get(field) {
                out.insert(field.to_string(), value.clone());
            }
        }
        // Coarsen `time`, then synthesize `modified` from the
        // coarsened map — npm packuments nest `modified` under `time`
        // and pacquet's resolver reads it at the top level.
        if let Some(time) = obj.get("time").and_then(Value::as_object) {
            let time = coarsen_time_map(time, now);
            if let Some(modified) = time.get("modified") {
                out.insert("modified".to_string(), modified.clone());
            }
            out.insert("time".to_string(), Value::Object(time));
        }
        if let Some(versions) = obj.get("versions").and_then(Value::as_object) {
            let mut abbreviated_versions = serde_json::Map::with_capacity(versions.len());
            for (version_id, version_value) in versions {
                let Some(version_obj) = version_value.as_object() else { continue };
                let mut trimmed = serde_json::Map::new();
                for &field in ABBREVIATED_VERSION_FIELDS {
                    if let Some(value) = version_obj.get(field) {
                        trimmed.insert(field.to_string(), value.clone());
                    }
                }
                trim_dist_fields(&mut trimmed);
                abbreviated_versions.insert(version_id.clone(), Value::Object(trimmed));
            }
            out.insert("versions".to_string(), Value::Object(abbreviated_versions));
        }
    }
    Value::Object(out)
}

/// Trim `dist` subfields the resolver and installer never read:
///
/// * `npm-signature` — the legacy PGP detached signature. npm stopped
///   populating it years ago in favour of the ECDSA `signatures`, and
///   nothing in pnpm or pacquet reads it.
/// * `shasum` — the legacy sha1 hash, redundant once `integrity` (SRI)
///   is present. "Present" mirrors pnpm's `getIntegrity` truthiness
///   check (`if (dist.integrity)`): a non-empty string. An absent,
///   empty, or non-string `integrity` keeps `shasum` so pnpm's
///   sha1 fallback still has a hash (pre-2017 publishes).
///
/// `dist.signatures` (the ECDSA registry signatures) is deliberately
/// preserved: it binds `name@version:integrity` to the upstream
/// registry's key and is the input to a potential client-side
/// install-time verification on the pnpr path.
///
/// `unpackedSize` and `fileCount` are preserved: pacquet reads both
/// off the resolver-fetched manifest — `unpackedSize` sizes the
/// decompression buffer, and together they form the download's
/// queueing priority (the estimated pipeline work that starts the
/// most expensive tarballs first).
fn trim_dist_fields(version: &mut serde_json::Map<String, Value>) {
    let Some(dist) = version.get_mut("dist").and_then(Value::as_object_mut) else {
        return;
    };
    dist.remove("npm-signature");
    if dist.get("integrity").and_then(Value::as_str).is_some_and(|integrity| !integrity.is_empty())
    {
        dist.remove("shasum");
    }
}

/// Shrink a packument `time` map by dropping precision the resolvers
/// don't need: seconds come off every timestamp, and entries older
/// than [`TIME_PRECISION_HORIZON_DAYS`] lose the time-of-day entirely
/// (down to the bare `YYYY-MM-DD`). Responses go out uncompressed, so
/// every character dropped is a byte off the wire.
///
/// Both reduced forms stay parseable by pnpm (`new Date`) and pacquet
/// ([`pacquet_resolving_resolver_base::parse_packument_timestamp`]).
/// Values are rounded *up* (see [`coarsen_timestamp`]) so the
/// maturity- and trust-checks that read them stay fail-safe.
/// Non-timestamp entries (the reserved `unpublished` object) and any
/// value pnpr can't parse as RFC 3339 pass through untouched.
fn coarsen_time_map(
    time: &serde_json::Map<String, Value>,
    now: DateTime<Utc>,
) -> serde_json::Map<String, Value> {
    let horizon = now - Duration::days(TIME_PRECISION_HORIZON_DAYS);
    let mut out = serde_json::Map::with_capacity(time.len());
    for (key, value) in time {
        let coarsened = value.as_str().and_then(|raw| coarsen_timestamp(raw, horizon));
        out.insert(key.clone(), coarsened.map_or_else(|| value.clone(), Value::String));
    }
    out
}

/// Re-render one RFC 3339 timestamp at reduced precision, **rounding
/// up**: a bare date (the next day, unless already midnight) when it
/// predates `horizon`, otherwise the next whole minute (unless already
/// on a minute boundary). Rounding up keeps the coarsened value at or
/// after the real publish time, so `minimumReleaseAge` and trust
/// checks can only ever read a version as *newer* than it is — the
/// fail-safe direction (a too-new version is never coarsened into
/// looking mature). Returns `None` for strings that aren't RFC 3339 so
/// the caller keeps the original verbatim.
fn coarsen_timestamp(raw: &str, horizon: DateTime<Utc>) -> Option<String> {
    let parsed = DateTime::parse_from_rfc3339(raw).ok()?.with_timezone(&Utc);
    if parsed < horizon {
        let date = parsed.date_naive();
        let rounded =
            if parsed == date.and_hms_opt(0, 0, 0)?.and_utc() { date } else { date.succ_opt()? };
        Some(rounded.format("%Y-%m-%d").to_string())
    } else {
        let minute = parsed.with_second(0)?.with_nanosecond(0)?;
        let rounded = if parsed == minute { minute } else { minute + Duration::minutes(1) };
        Some(rounded.format("%Y-%m-%dT%H:%MZ").to_string())
    }
}

#[cfg(test)]
mod tests;
