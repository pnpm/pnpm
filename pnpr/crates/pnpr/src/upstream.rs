use crate::{
    config::{RedactedHeaders, UplinkConfig},
    error::{RegistryError, Result},
    package_name::PackageName,
};
use chrono::{DateTime, Timelike, Utc};
use pacquet_network::ThrottledClient;
use reqwest::{
    StatusCode,
    header::{self, HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Wraps a shared [`ThrottledClient`] (so the registry inherits pnpm's
/// tuned reqwest defaults: `User-Agent: pnpm`, HTTP/1.1, hickory DNS,
/// pool/timeout tuning, concurrency semaphore, and per-registry TLS
/// routing if it's ever wired in later) and adds the per-uplink glue a
/// proxy needs: building the upstream URL, applying verdaccio's
/// `timeout`/`max_fails`/`fail_timeout` knobs, and fishing the packument
/// or tarball response out of it.
#[derive(Clone)]
pub struct Upstream {
    client: Arc<ThrottledClient>,
    base: String,
    /// The configured uplink name (the YAML `uplinks:` key). Surfaced in
    /// client-facing errors so an open circuit names the uplink rather
    /// than leaking its upstream URL.
    name: String,
    /// Resolved per-uplink request headers (auth + custom) attached to
    /// every fetch. Empty for an uplink with no `auth:`/`headers:`.
    headers: HeaderMap,
    /// Per-request deadline (verdaccio's `timeout`).
    timeout: Duration,
    /// Per-uplink packument freshness window (verdaccio's `maxage`), or
    /// `None` to defer to the global [`crate::config::Config::packument_ttl`].
    maxage: Option<Duration>,
    /// Whether tarballs from this uplink are written to the local mirror
    /// (verdaccio's `cache`).
    cache: bool,
    /// Shared failure tracker implementing verdaccio's
    /// `max_fails`/`fail_timeout` circuit breaker. Behind an [`Arc`] so
    /// every clone of this `Upstream` (the registry holds one per uplink
    /// and clones it per request) updates the same counters.
    breaker: Arc<CircuitBreaker>,
}

impl fmt::Debug for Upstream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Upstream")
            .field("client", &self.client)
            .field("base", &self.base)
            .field("name", &self.name)
            .field("headers", &RedactedHeaders(&self.headers))
            .field("timeout", &self.timeout)
            .field("maxage", &self.maxage)
            .field("cache", &self.cache)
            .field("breaker", &self.breaker)
            .finish()
    }
}

/// Verdaccio's `max_fails` / `fail_timeout` circuit breaker. After
/// `max_fails` consecutive failures the uplink is considered down and
/// requests short-circuit, until `fail_timeout` elapses since the last
/// failure — a single probe is then allowed through, and its success
/// resets the breaker or its failure restarts the cooldown.
///
/// The half-open probe is gated by the cooldown window itself: admitting
/// a probe advances `last_failure` to now, so concurrent callers in that
/// window stay short-circuited and a recovering upstream sees one probe
/// rather than a stampede. Crucially this can't *stick* — a probe whose
/// request is cancelled or dropped before it reports back simply lets the
/// window lapse, after which the next caller probes. A sticky in-flight
/// flag would deadlock the breaker on a cancelled request.
///
/// The counter and the timestamp live behind one [`Mutex`] so a threshold
/// check and its timestamp can never be observed half-updated. The lock
/// is held only for trivial field reads/writes (never across the network
/// request), so contention is negligible; a poisoned lock is recovered
/// rather than propagated as a panic, keeping a failing uplink from
/// taking the registry down.
#[derive(Debug)]
struct CircuitBreaker {
    max_fails: u32,
    fail_timeout: Duration,
    state: Mutex<BreakerState>,
}

#[derive(Debug, Default)]
struct BreakerState {
    failed_requests: u32,
    last_failure: Option<Instant>,
}

impl CircuitBreaker {
    fn new(max_fails: u32, fail_timeout: Duration) -> Self {
        Self { max_fails, fail_timeout, state: Mutex::new(BreakerState::default()) }
    }

    /// Recover the guard from a poisoned lock instead of panicking: the
    /// breaker only ever holds plain counters, so the worst a poisoned
    /// guard carries is a stale failure count, never an invariant break.
    fn lock(&self) -> std::sync::MutexGuard<'_, BreakerState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Try to admit one request. Returns `true` while under `max_fails`
    /// consecutive failures (or when the breaker is disabled). Once the
    /// `fail_timeout` cooldown has elapsed it admits one probe per window:
    /// the admitting caller advances the cooldown, so concurrent callers
    /// stay short-circuited until the next window opens. `max_fails == 0`
    /// disables the breaker entirely.
    fn try_acquire(&self) -> bool {
        let mut state = self.lock();
        if self.max_fails == 0 || state.failed_requests < self.max_fails {
            return true;
        }
        // Tripped: stay open until the cooldown since the last failure
        // lapses. (`failed_requests >= max_fails` always implies a
        // recorded `last_failure`, so the `None` arm is unreachable; it
        // fails open for safety.)
        let cooled_down = state.last_failure.is_none_or(|at| at.elapsed() >= self.fail_timeout);
        if !cooled_down {
            return false;
        }
        // Admit this probe and re-arm the cooldown so the next caller is
        // held back for another `fail_timeout`. If the probe never reports
        // back (cancelled mid-request), the window simply lapses and the
        // following caller probes — the breaker can't deadlock.
        state.last_failure = Some(Instant::now());
        true
    }

    fn record_success(&self) {
        *self.lock() = BreakerState::default();
    }

    fn record_failure(&self) {
        let mut state = self.lock();
        state.failed_requests = state.failed_requests.saturating_add(1);
        state.last_failure = Some(Instant::now());
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
    /// Build an uplink client from its name (the YAML `uplinks:` key) and
    /// resolved [`UplinkConfig`], baking in the per-uplink
    /// `timeout`/`maxage`/`cache` knobs and arming the
    /// `max_fails`/`fail_timeout` circuit breaker.
    pub fn new(name: &str, config: &UplinkConfig) -> Self {
        Self {
            client: Arc::new(ThrottledClient::new_for_installs()),
            base: config.url.clone(),
            name: name.to_string(),
            headers: config.headers.clone(),
            timeout: config.timeout,
            maxage: config.maxage,
            cache: config.cache,
            breaker: Arc::new(CircuitBreaker::new(config.max_fails, config.fail_timeout)),
        }
    }

    /// Per-uplink packument freshness window (`maxage`), or `None` to
    /// defer to the global [`crate::config::Config::packument_ttl`].
    pub fn maxage(&self) -> Option<Duration> {
        self.maxage
    }

    /// Whether tarballs from this uplink should be written to the local
    /// mirror (`cache: true`). When `false` the caller streams the body
    /// straight through without caching it.
    pub fn caches(&self) -> bool {
        self.cache
    }

    /// Fetch a package's packument, conditionally when `validators`
    /// carries an `ETag`/`Last-Modified`. A `304 Not Modified` short-
    /// circuits to [`PackumentFetch::NotModified`] without a body, so the
    /// caller can keep serving its cached copy — the bandwidth win on a
    /// stale-but-current packument.
    ///
    /// Returns [`RegistryError::UpstreamUnavailable`] without hitting the
    /// network when the circuit breaker is open.
    pub async fn fetch_packument(
        &self,
        name: &PackageName,
        validators: &CacheValidators,
    ) -> Result<PackumentFetch> {
        self.ensure_available()?;
        let url = format!("{}/{}", self.base.trim_end_matches('/'), name.as_str());
        let client = self.client.acquire_for_url(&url).await;
        let mut request = client.get(&url).timeout(self.timeout).headers(self.headers.clone());
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
        let response = self.run(request, &url).await?;
        if response.status() == StatusCode::NOT_FOUND {
            // A 404 is an authoritative answer, not an uplink failure.
            self.breaker.record_success();
            return Ok(PackumentFetch::NotFound);
        }
        // Only honor a `304` if we actually sent a conditional header. A
        // `304` to an unconditional request is a misbehaving upstream and
        // carries no body to serve, so let it fall through to `checked`
        // and surface as an upstream status error rather than reusing a
        // (possibly nonexistent) cached copy.
        if sent_conditional && response.status() == StatusCode::NOT_MODIFIED {
            self.breaker.record_success();
            return Ok(PackumentFetch::NotModified);
        }
        let response = self.checked(response, &url).await?;
        let validators = CacheValidators::from_headers(response.headers());
        let bytes = response.bytes().await.map_err(|source| {
            self.breaker.record_failure();
            RegistryError::Upstream { url: url.clone(), source }
        })?;
        self.breaker.record_success();
        Ok(PackumentFetch::Modified(FetchedPackument { bytes: bytes.to_vec(), validators }))
    }

    /// Send the tarball request and return the streaming
    /// [`reqwest::Response`] so the caller can pipe the body straight
    /// to the client without buffering. Status and 404 handling
    /// happen here before any bytes are forwarded.
    ///
    /// Returns [`RegistryError::UpstreamUnavailable`] without hitting the
    /// network when the circuit breaker is open.
    pub async fn fetch_tarball_response(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<FetchOutcome<reqwest::Response>> {
        self.ensure_available()?;
        let url = format!("{}/{}/-/{}", self.base.trim_end_matches('/'), name.as_str(), filename);
        let client = self.client.acquire_for_url(&url).await;
        let request = client.get(&url).timeout(self.timeout).headers(self.headers.clone());
        let response = self.run(request, &url).await?;
        if response.status() == StatusCode::NOT_FOUND {
            self.breaker.record_success();
            return Ok(FetchOutcome::NotFound);
        }
        let response = self.checked(response, &url).await?;
        // Success here covers only headers; a mid-stream body failure is
        // the caller's to observe. Recording success on a clean status is
        // what verdaccio does too.
        self.breaker.record_success();
        Ok(FetchOutcome::Ok(response))
    }

    /// Fail fast with [`RegistryError::UpstreamUnavailable`] when the
    /// breaker is open, so callers never pay a request to a known-down
    /// uplink.
    fn ensure_available(&self) -> Result<()> {
        if self.breaker.try_acquire() {
            return Ok(());
        }
        Err(RegistryError::UpstreamUnavailable { uplink: self.name.clone() })
    }

    /// Send a built request, mapping a transport error to
    /// [`RegistryError::Upstream`] and counting it against the breaker.
    async fn run(&self, request: reqwest::RequestBuilder, url: &str) -> Result<reqwest::Response> {
        request.send().await.map_err(|source| {
            self.breaker.record_failure();
            RegistryError::Upstream { url: url.to_string(), source }
        })
    }

    /// Pass a successful response through; map any non-success status to
    /// [`RegistryError::UpstreamStatus`]. Only a `5xx` counts against the
    /// breaker — a non-404 `4xx` is an authoritative client error (auth,
    /// rate-limit, bad request), not an availability signal, so it leaves
    /// the breaker untouched rather than opening the circuit and masking
    /// the real status behind a `503`. The `404`/`304` paths are handled
    /// by the callers before reaching here.
    async fn checked(&self, response: reqwest::Response, url: &str) -> Result<reqwest::Response> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        if status.is_server_error() {
            self.breaker.record_failure();
        }
        let body = response.text().await.unwrap_or_default();
        Err(RegistryError::UpstreamStatus { url: url.to_string(), status: status.as_u16(), body })
    }
}

/// Rewrite every `dist.tarball` in `value` to a URL served by *this*
/// registry instead of whatever URL the source put there. The new URL is
/// `{public_url}/{pkg}/-/{basename}`, where `basename` is the last
/// `/`-separated segment of the original tarball URL. This handles both
/// npm's canonical `/{pkg}/-/{basename}` shape and verdaccio's
/// `/{scope}/{name}/-/{scope}/{filename}` shape uniformly — we only look
/// at the basename, never at the path prefix.
///
/// The basename is preserved verbatim rather than reconstructed from the
/// version, so a non-canonical tarball name (e.g. esprima-fb's zero-padded
/// `esprima-fb-3001.0001.0000-dev-harmony-fb.tgz` for version
/// `3001.1.0-dev-harmony-fb`) survives into the client's lockfile and is
/// fetched back from the path the upstream actually hosts. The tarball
/// endpoint binds each request to a version's declared `dist.integrity`
/// (see `serve_tarball`), so a preserved name can't smuggle in unverified
/// bytes.
///
/// Walks both packument shape (`{ "versions": { v: { dist: ... } } }`)
/// and single-version manifest shape (`{ dist: ... }` at the top level)
/// so a single helper covers both endpoints.
pub fn rewrite_tarball_urls(value: &mut Value, pkg: &PackageName, public_url: &str) {
    let public_url = public_url.trim_end_matches('/');
    if let Some(versions) = value.get_mut("versions").and_then(Value::as_object_mut) {
        for manifest in versions.values_mut() {
            rewrite_dist_tarball(manifest, pkg, public_url);
        }
    }
    rewrite_dist_tarball(value, pkg, public_url);
}

fn rewrite_dist_tarball(value: &mut Value, pkg: &PackageName, public_url: &str) {
    // Every string `dist.tarball` must be rewritten to a route on *this*
    // server, where integrity and OSV are enforced — never passed through.
    // When the upstream URL has no usable basename (e.g. it ends in `/`), fall
    // back to the version-derived canonical name (the manifest carries its own
    // `version`) so a malformed URL still points at pnpr (and 404s there)
    // rather than directing the client at an arbitrary upstream host.
    let fallback = value
        .get("version")
        .and_then(Value::as_str)
        .map(|version| pkg.tarball_name_for_version(version));
    let Some(dist) = value.get_mut("dist").and_then(Value::as_object_mut) else {
        return;
    };
    let Some(tarball_value) = dist.get_mut("tarball") else { return };
    if !tarball_value.is_string() {
        return;
    }
    let filename = tarball_value
        .as_str()
        .and_then(tarball_basename)
        .map(str::to_owned)
        .or(fallback)
        .unwrap_or_default();
    *tarball_value = Value::String(format!("{public_url}/{}/-/{filename}", pkg.as_str()));
}

/// The tarball filename a `dist.tarball` URL points at: the final path
/// segment, with any `?query`/`#fragment` stripped. This basename is the
/// trust key shared by the rewritten public URL ([`rewrite_tarball_urls`])
/// and the serve-time version match (`expected_tarball_dist`), so both
/// must derive it identically — including for query-bearing URLs (signed
/// CDN links), where the query is not part of the route path a client
/// later requests. Returns `None` for a URL whose path ends in `/`.
pub fn tarball_basename(url: &str) -> Option<&str> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit('/').next().filter(|segment| !segment.is_empty())
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
    let horizon = now - chrono::Duration::days(TIME_PRECISION_HORIZON_DAYS);
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
        let rounded = if parsed == minute { minute } else { minute + chrono::Duration::minutes(1) };
        Some(rounded.format("%Y-%m-%dT%H:%MZ").to_string())
    }
}

#[cfg(test)]
mod tests;
