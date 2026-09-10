pub use auth::{
    AuthHeaders, AuthHeadersByScope, DEFAULT_REGISTRY_SCOPE, MetadataCacheScope, UpstreamRouteHook,
    base64_encode, base64_encode_bytes, hide_auth_information, is_url_secure_for_credentials,
    nerf_dart, normalize_auth_key, redact_and_sanitize, redact_and_sanitize_multiline,
    redact_url_credentials, redact_url_for_display,
};
pub use client_builder::{RedirectGuard, default_network_concurrency, native_dns_resolver};
pub use limited_body::{LimitedBody, read_limited_body};
pub use proxy::{NoProxySetting, ProxyConfig, ProxyError};
pub use retry::{
    RetryOpts, retry_async, send_with_retry, send_with_retry_at_priority, should_retry_status,
};
pub use tls::{PerRegistryTls, RegistryTls, TlsConfig, TlsError};
pub use token_helper::{TokenHelperOutput, TokenHelperRunner};
pub use url_encoding::{encode_package_name, encode_uri_component, percent_decode_str};

mod auth;
mod limited_body;
mod priority_semaphore;
mod proxy;
mod retry;
#[cfg(test)]
mod tests;
mod tls;
mod token_helper;

mod url_encoding;

use priority_semaphore::{Permit, PrioritySemaphore};
use proxy::{NoProxyMatcher, parse_proxy_url, strip_userinfo};
use reqwest::{
    Certificate, Client, Identity, Proxy,
    dns::{Addrs, Name, Resolve, Resolving},
    header::{HeaderMap, HeaderValue, USER_AGENT},
};
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    ops::Deref,
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Fallback `User-Agent` for the install client's no-config
/// constructors ([`ThrottledClient::new_for_installs`]) and for the case where a
/// configured user-agent string cannot be encoded as an HTTP header
/// value.
///
/// Production installs override this with the value resolved by
/// `pnpm-config` (`userAgent`, defaulting to the
/// `pnpm/<version> npm/? node/? <platform> <arch>` format). The leading
/// `pnpm` token is what UA-keyed allow / rate-limit rules expect, so any
/// rule that lets pnpm through also lets this build through.
///
/// A default `reqwest::Client` sends *no* User-Agent at all, which
/// some registry CDNs and corporate WAFs treat as a bot signature and
/// either block at the edge or terminate mid-handshake (surfacing as a
/// generic "error sending request for url" with no body to look at).
/// The install client therefore always sends one.
pub const DEFAULT_USER_AGENT: &str = "pnpm";

/// Permit priority used by [`ThrottledClient::acquire`] /
/// [`ThrottledClient::acquire_for_url`] for callers that don't pass an
/// explicit one. Marks the request as latency class — packument and
/// other metadata fetches that gate resolution progress — served FIFO
/// and preferred over size-prioritized downloads beyond the downloads'
/// reserved share of the pool (see the `priority_semaphore` module
/// docs for the two-class grant policy).
pub const UNPRIORITIZED: u64 = u64::MAX;

/// Priority sentinel for the background class: metadata fetches whose
/// deadline is the end of the install rather than the next resolution
/// step — the lockfile-verification fan-out. A background request is
/// granted a slot only when no [`UNPRIORITIZED`] (latency-class)
/// request is queued, so bulk verification never queue-jumps the
/// resolver's critical-path packument fetches (see the
/// `priority_semaphore` module docs for the full grant policy).
pub const BACKGROUND: u64 = u64::MAX - 1;

/// Highest priority a throughput-class (download) request may carry.
/// Callers that derive a priority from untrusted size hints must clamp
/// to this, so a saturated estimate can never collide with the
/// [`BACKGROUND`] or [`UNPRIORITIZED`] sentinels and change the
/// request's class.
pub const MAX_THROUGHPUT_PRIORITY: u64 = BACKGROUND - 1;

/// Default network-inactivity timeout in milliseconds: the
/// `fetchTimeout` default of `60000`. Source of truth for
/// `pnpm-config`'s `default_fetch_timeout`.
pub const DEFAULT_FETCH_TIMEOUT_MS: u64 = 60_000;

/// Default slow-metadata-request warning threshold in milliseconds: the
/// `fetchWarnTimeoutMs` default of `10000`.
pub const DEFAULT_FETCH_WARN_TIMEOUT_MS: u64 = 10_000;

/// Default minimum average tarball download speed in KiB/s: the
/// `fetchMinSpeedKiBps` default of `50`.
pub const DEFAULT_FETCH_MIN_SPEED_KI_BPS: u64 = 50;

/// Tunable network knobs threaded into the install client: the
/// `networkConcurrency`, `fetchTimeout`, `fetchWarnTimeoutMs`,
/// `fetchMinSpeedKiBps`, and `userAgent` settings.
/// `pnpm-config` owns their defaults and override sources
/// (`pnpm-workspace.yaml`, `PNPM_CONFIG_*`, CLI flags) and hands the
/// resolved values here.
#[derive(Debug, Clone)]
pub struct NetworkSettings {
    /// Maximum number of concurrent in-flight network requests — the
    /// semaphore size. Default: [`default_network_concurrency`].
    pub network_concurrency: usize,

    /// How long a request may make no progress before it fails, applied
    /// as both reqwest's read timeout and its connect timeout. A
    /// download that keeps receiving data runs as long as it needs.
    /// Default: [`DEFAULT_FETCH_TIMEOUT_MS`].
    pub fetch_timeout: Duration,

    /// Successful metadata requests slower than this emit a warning.
    /// Default: [`DEFAULT_FETCH_WARN_TIMEOUT_MS`].
    pub fetch_warn_timeout: Duration,

    /// Successful tarball downloads whose average speed falls below this
    /// value emit a warning. Default: [`DEFAULT_FETCH_MIN_SPEED_KI_BPS`].
    pub fetch_min_speed_ki_bps: u64,

    /// Value of the `User-Agent` header sent on every request.
    /// Default: [`DEFAULT_USER_AGENT`].
    pub user_agent: String,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        NetworkSettings {
            network_concurrency: default_network_concurrency(),
            fetch_timeout: Duration::from_millis(DEFAULT_FETCH_TIMEOUT_MS),
            fetch_warn_timeout: Duration::from_millis(DEFAULT_FETCH_WARN_TIMEOUT_MS),
            fetch_min_speed_ki_bps: DEFAULT_FETCH_MIN_SPEED_KI_BPS,
            user_agent: DEFAULT_USER_AGENT.to_string(),
        }
    }
}

/// Wrapper around [`Client`] with a concurrent request limit enforced
/// by a priority-ordered semaphore (`priority_semaphore` module).
///
/// Holds a default [`Client`] for the top-level proxy / TLS config
/// plus an optional map of per-registry clients keyed by nerf-darted
/// URI. [`Self::acquire_for_url`] picks the right client based on the
/// request URL (a 5-step fallback), and [`Self::acquire`] always uses
/// the default client. The semaphore is shared across both — bounding
/// the total
/// concurrent socket count regardless of which registry a request
/// targets.
///
/// When the pool saturates, freed slots are granted by a two-class
/// policy (see the `priority_semaphore` module docs): requests
/// acquired without an explicit priority ([`Self::acquire`],
/// [`Self::acquire_for_url`]) form the FIFO latency class (typically
/// metadata fetches gating resolution progress), while downloads pass
/// their estimated pipeline work through
/// [`Self::acquire_for_url_with_priority`] and are guaranteed a
/// reserved share of the pool, granted most-expensive-first — so the
/// longest download jobs start early and neither class starves the
/// other.
#[derive(Debug)]
pub struct ThrottledClient {
    semaphore: PrioritySemaphore,
    default_clients: ClientPair,
    /// Per-registry clients keyed by nerf-darted URI. Empty when no
    /// `//host/:cert=…` / `:key=…` / `:ca=…` / `:cafile=…` /
    /// `:certfile=…` / `:keyfile=…` `.npmrc` entries are present —
    /// in which case `acquire_for_url` short-circuits to the default
    /// client without paying the routing cost.
    per_registry: tls::PerRegistryMap<ClientPair>,
    /// Per-origin socket cap (the `maxSockets` setting). `None` (the
    /// default) leaves the per-origin socket count bounded only by
    /// `semaphore`; see [`HostSocketLimit`].
    host_socket_limit: Option<HostSocketLimit>,
    fetch_warn_timeout: Duration,
    fetch_min_speed_ki_bps: u64,
    warning_handler: std::sync::RwLock<fn(&str)>,
}

#[derive(Debug)]
struct ClientPair {
    follow_redirects: Client,
    no_redirects: Client,
}

impl ClientPair {
    fn select(&self, follow_redirects: bool) -> &Client {
        if follow_redirects { &self.follow_redirects } else { &self.no_redirects }
    }
}

/// Per-origin concurrent-connection cap, mirroring undici's `connections`
/// option (the `maxSockets` setting pnpm applies per registry origin).
///
/// Each distinct `scheme://host[:port]` origin gets its own [`Semaphore`] of
/// `max` permits, minted on first request to that origin. Acquired *before*
/// the global [`ThrottledClient::semaphore`] so a request waiting on a
/// saturated origin does not hold a global concurrency slot — that would let a
/// burst to one origin hoard every global permit and starve other origins.
#[derive(Debug)]
struct HostSocketLimit {
    max: NonZeroUsize,
    per_origin: Mutex<HashMap<String, Arc<Semaphore>>>,
}

impl HostSocketLimit {
    /// Acquire an owned permit for `url`'s origin, or `None` when `url` has no
    /// parseable `scheme://host` (in which case the request falls back to the
    /// global concurrency bound alone).
    async fn acquire(&self, url: &str) -> Option<OwnedSemaphorePermit> {
        let origin = origin_of(url)?;
        // Lock only long enough to look up (or mint) the origin's semaphore and
        // clone its `Arc` — never held across the `.await` below.
        let semaphore = {
            let mut map = self.per_origin.lock().expect("host-socket-limit mutex poisoned");
            Arc::clone(
                map.entry(origin).or_insert_with(|| Arc::new(Semaphore::new(self.max.get()))),
            )
        };
        Some(semaphore.acquire_owned().await.expect("host-socket semaphore is never closed"))
    }
}

/// The `scheme://host[:port]` origin of `url`, or `None` when it has no host.
/// `url` strips a scheme-default port while parsing (`https://host:443` parses
/// with `port() == None`), so an explicit default and the implicit form map to
/// the same origin key and a `:443` / `:80` variation cannot fragment the
/// per-origin socket cap; a non-default port (`https://host:8443`) stays
/// distinct — matching undici's per-origin keying.
fn origin_of(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    Some(match parsed.port() {
        Some(port) => format!("{}://{host}:{port}", parsed.scheme()),
        None => format!("{}://{host}", parsed.scheme()),
    })
}

/// RAII guard returned from [`ThrottledClient::acquire`]. Holds a
/// semaphore permit alongside a reference to the underlying
/// [`Client`]; the permit is released when the guard is dropped.
///
/// The guard derefs to [`Client`] so callers can chain
/// `guard.get(url).send().await?.json().await?` (or any other
/// reqwest method) directly. **Holding the guard across the body
/// await is the point of the API.** A request's socket FD lives
/// from `connect` all the way through body streaming; dropping the
/// permit when `.send()` returns (right after headers arrive, with
/// the body still pending) means the semaphore stops bounding the
/// real concurrent socket count. Under `try_join_all` fan-out the
/// next batch of permits then `connect()` while previous bodies are
/// still draining, and the per-process FD count overruns the
/// platform limit — surfacing as `EMFILE` "too many open files".
pub struct ThrottledClientGuard<'a> {
    permit: Permit,
    /// The per-origin `maxSockets` permit, held for the same request lifetime
    /// as `permit`. `None` when no `maxSockets` cap is configured or the URL
    /// had no parseable origin.
    host_permit: Option<OwnedSemaphorePermit>,
    client: &'a Client,
}

/// A response that retains the global and per-origin permits through body reads.
pub struct ThrottledResponse {
    response: reqwest::Response,
    _permit: Permit,
    _host_permit: Option<OwnedSemaphorePermit>,
    body_timeout: Duration,
    received_at: Instant,
}

impl ThrottledClientGuard<'_> {
    /// Transfer both concurrency permits to the response body owner.
    #[must_use]
    pub fn retain_for_body(
        self,
        response: reqwest::Response,
        body_timeout: Duration,
    ) -> ThrottledResponse {
        ThrottledResponse {
            response,
            _permit: self.permit,
            _host_permit: self.host_permit,
            body_timeout,
            received_at: Instant::now(),
        }
    }
}

impl ThrottledResponse {
    pub async fn bytes(self) -> Result<bytes::Bytes, reqwest::Error> {
        let Self { response, _permit, _host_permit, .. } = self;
        response.bytes().await
    }

    /// Buffer at most one channel chunk. The producer deadline and cancellation
    /// remain active even while the consumer stops polling the stream.
    pub fn bytes_stream(
        mut self,
    ) -> impl futures_util::Stream<Item = std::io::Result<bytes::Bytes>> + Send {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let (completion_sender, completion) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let remaining = self.body_timeout.saturating_sub(self.received_at.elapsed());
            let result = tokio::select! {
                () = sender.closed() => Ok(()),
                result = tokio::time::timeout(remaining, async {
                    while let Some(chunk) = self.response.chunk().await.map_err(std::io::Error::other)? {
                        if sender.send(chunk).await.is_err() {
                            return Ok(());
                        }
                    }
                    Ok(())
                }) => result.unwrap_or_else(|_| Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "upstream response body deadline exceeded",
                ))),
            };
            drop(self);
            let _ = completion_sender.send(result);
        });
        futures_util::stream::try_unfold(
            (receiver, completion),
            |(mut receiver, completion)| async move {
                if let Some(chunk) = receiver.recv().await {
                    return Ok(Some((chunk, (receiver, completion))));
                }
                completion.await.map_err(std::io::Error::other)??;
                Ok::<_, std::io::Error>(None)
            },
        )
    }
}

impl Deref for ThrottledResponse {
    type Target = reqwest::Response;

    fn deref(&self) -> &Self::Target {
        &self.response
    }
}

impl std::fmt::Debug for ThrottledResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ThrottledResponse")
            .field("response", &self.response)
            .field("body_timeout", &self.body_timeout)
            .field("received_at", &self.received_at)
            .finish_non_exhaustive()
    }
}

impl Deref for ThrottledClientGuard<'_> {
    type Target = Client;

    fn deref(&self) -> &Client {
        self.client
    }
}

impl ThrottledClient {
    /// Replace the sink used by successful slow-fetch warnings.
    pub fn set_warning_handler(&self, handler: fn(&str)) {
        *self.warning_handler.write().expect("warning-handler lock poisoned") = handler;
    }

    /// Emit a successful slow-fetch warning through the configured sink.
    pub fn warn(&self, message: &str) {
        let handler = *self.warning_handler.read().expect("warning-handler lock poisoned");
        handler(message);
    }

    /// The `fetchWarnTimeoutMs` threshold configured for this client.
    pub fn fetch_warn_timeout(&self) -> Duration {
        self.fetch_warn_timeout
    }

    /// The `fetchMinSpeedKiBps` threshold configured for this client.
    pub fn fetch_min_speed_ki_bps(&self) -> u64 {
        self.fetch_min_speed_ki_bps
    }

    /// Acquire a permit and return a guard granting access to the
    /// underlying [`Client`]. The permit is released when the guard
    /// is dropped, so callers control how long the request "counts"
    /// against [`default_network_concurrency`] — typically the full
    /// `send + body-consume` lifetime, not just `.send()`.
    pub async fn acquire(&self) -> ThrottledClientGuard<'_> {
        let permit = self.semaphore.acquire(UNPRIORITIZED).await;
        ThrottledClientGuard {
            permit,
            host_permit: None,
            client: &self.default_clients.follow_redirects,
        }
    }

    /// Install a per-origin socket cap (the `maxSockets` setting) on this
    /// client. `None` or `Some(0)` leaves the client uncapped — the per-origin
    /// socket count then stays bounded only by the global concurrency
    /// semaphore. Chained onto [`Self::for_installs`] at the install call
    /// sites; the client's other constructors leave it uncapped.
    #[must_use]
    pub fn with_max_sockets_per_host(mut self, max_sockets: Option<usize>) -> Self {
        self.host_socket_limit = max_sockets
            .and_then(NonZeroUsize::new)
            .map(|max| HostSocketLimit { max, per_origin: Mutex::new(HashMap::new()) });
        self
    }

    /// Acquire a permit and return a guard granting access to the
    /// per-registry [`Client`] that matches `url`'s nerf-darted form
    /// (falling back to the default client when no override matches).
    /// The semaphore is shared across all clients, so total concurrent
    /// socket count stays bounded by [`default_network_concurrency`]
    /// regardless of which registry the request targets.
    ///
    /// Per-URL routing uses a 5-step fallback: exact, then nerf-darted,
    /// then host without port, then progressively shorter path prefixes,
    /// then a recursive retry without port. When no per-registry overrides
    /// are configured (the common case), the routing table is empty
    /// and the lookup short-circuits to the default client.
    ///
    /// Takes `url` as `&str` so callers don't have to round-trip
    /// `format!("{registry}{name}")` strings through `Url::parse`
    /// just to satisfy the type signature — the lookup itself works
    /// on the raw string form.
    pub async fn acquire_for_url(&self, url: &str) -> ThrottledClientGuard<'_> {
        self.acquire_for_url_with_priority(url, UNPRIORITIZED).await
    }

    /// [`Self::acquire_for_url`], but queueing behind the saturated
    /// pool at an explicit `priority` instead of [`UNPRIORITIZED`].
    /// [`BACKGROUND`] selects the background class (bulk verification
    /// metadata); any other value selects the throughput class.
    /// Tarball downloads pass their estimated pipeline work (0 when
    /// unknown) so that freed slots go to the most expensive pending
    /// archive first — the longest download+extract jobs start
    /// earliest and never end up running alone after the small ones
    /// drained.
    pub async fn acquire_for_url_with_priority(
        &self,
        url: &str,
        priority: u64,
    ) -> ThrottledClientGuard<'_> {
        self.acquire_for_url_with_priority_and_redirects(url, priority, true).await
    }

    /// [`Self::acquire_for_url_with_priority`] using a client that returns the
    /// first redirect response instead of following it.
    pub async fn acquire_for_url_without_redirects_with_priority(
        &self,
        url: &str,
        priority: u64,
    ) -> ThrottledClientGuard<'_> {
        self.acquire_for_url_with_priority_and_redirects(url, priority, false).await
    }
}

pub struct SecureAuthResponse {
    pub status: reqwest::StatusCode,
    pub body: Vec<u8>,
    pub body_truncated: bool,
    pub url: String,
}

fn ignore_warning(_: &str) {}

impl<Inner> CappedDnsResolver<Inner> {
    fn new(inner: Inner, concurrency: NonZeroUsize) -> Self {
        Self { inner: Arc::new(inner), permits: Arc::new(Semaphore::new(concurrency.get())) }
    }
}

impl<Inner> Resolve for CappedDnsResolver<Inner>
where
    Inner: Resolve + 'static,
{
    fn resolve(&self, name: Name) -> Resolving {
        let inner = Arc::clone(&self.inner);
        let permits = Arc::clone(&self.permits);
        Box::pin(async move {
            let _permit =
                permits.acquire_owned().await.expect("DNS concurrency semaphore is never closed");
            inner.resolve(name).await
        })
    }
}

/// Error surface of [`ThrottledClient::for_installs`]. Wraps either a
/// proxy URL failure or a TLS material failure — the caller gets one
/// error type to handle regardless of which side of `for_installs`
/// rejected the input.
#[derive(Debug, derive_more::Display, derive_more::Error, miette::Diagnostic)]
#[non_exhaustive]
pub enum ForInstallsError {
    #[diagnostic(transparent)]
    Proxy(#[error(source)] ProxyError),

    #[diagnostic(transparent)]
    Tls(#[error(source)] TlsError),

    /// `network_concurrency` resolved to `0`. A zero-permit semaphore
    /// would make every `acquire` block forever, hanging the install.
    /// pnpm rejects the same value — its `p-queue` throws
    /// `Expected concurrency to be a number from 1 and up` — so pacquet
    /// fails fast rather than deadlock.
    #[display("networkConcurrency must be at least 1")]
    ZeroNetworkConcurrency,

    /// reqwest rejected the assembled client configuration, with both
    /// the platform trust store and the bundled Mozilla roots. The
    /// platform attempt is the source (it is the one that describes
    /// the environment); the retry's own failure is spelled out too,
    /// since the two can differ.
    #[display("Failed to build the HTTP client (retry with bundled CA roots: {bundled})")]
    ClientBuild {
        #[error(source)]
        platform: reqwest::Error,
        bundled: reqwest::Error,
    },
}

impl From<ProxyError> for ForInstallsError {
    fn from(value: ProxyError) -> Self {
        ForInstallsError::Proxy(value)
    }
}

impl From<TlsError> for ForInstallsError {
    fn from(value: TlsError) -> Self {
        ForInstallsError::Tls(value)
    }
}

/// This is only necessary for tests.
impl Default for ThrottledClient {
    fn default() -> Self {
        ThrottledClient::new_for_installs()
    }
}

mod certificates;
use certificates::{
    TrustRoots, apply_tls, bundled_root_certs, load_node_extra_ca_certs, merge_tls,
};

mod client_builder;
use client_builder::{
    CappedDnsResolver, ClientBuildInputs, MAX_REDIRECT_HOPS, build_client_with_root_fallback,
    configured_proxy, is_redirect_status,
};

mod requests;

mod initialization;
