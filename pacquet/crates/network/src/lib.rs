mod auth;
mod priority_semaphore;
mod proxy;
mod retry;
#[cfg(test)]
mod tests;
mod tls;

pub use auth::{
    AuthHeaders, AuthHeadersByScope, DEFAULT_REGISTRY_SCOPE, MetadataCacheScope, UpstreamRouteHook,
    base64_encode, nerf_dart, redact_and_sanitize, redact_url_credentials,
};
pub use url_encoding::{encode_package_name, encode_uri_component};

mod url_encoding;
pub use proxy::{NoProxySetting, ProxyConfig, ProxyError};
pub use retry::{RetryOpts, retry_async, send_with_retry, should_retry_status};
pub use tls::{PerRegistryTls, RegistryTls, TlsConfig, TlsError};

use priority_semaphore::{Permit, PrioritySemaphore};
use proxy::{NoProxyMatcher, parse_proxy_url, strip_userinfo};
use reqwest::{
    Certificate, Client, Identity, Proxy,
    header::{HeaderMap, HeaderValue, USER_AGENT},
};
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    ops::Deref,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Fallback `User-Agent` for the install client's no-config
/// constructors ([`ThrottledClient::new_for_installs`],
/// [`ThrottledClient::from_client`]) and for the case where a
/// configured user-agent string cannot be encoded as an HTTP header
/// value.
///
/// Production installs override this with the value resolved by
/// `pacquet-config` (`userAgent`, defaulting to the
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

/// Default per-request timeout in milliseconds: the `fetchTimeout`
/// default of `60000`. Source of truth for `pacquet-config`'s
/// `default_fetch_timeout`.
pub const DEFAULT_FETCH_TIMEOUT_MS: u64 = 60_000;

/// Tunable network knobs threaded into the install client: the
/// `networkConcurrency`, `fetchTimeout`, and `userAgent` settings.
/// `pacquet-config` owns their defaults and override sources
/// (`pnpm-workspace.yaml`, `PNPM_CONFIG_*`, CLI flags) and hands the
/// resolved values here.
#[derive(Debug, Clone)]
pub struct NetworkSettings {
    /// Maximum number of concurrent in-flight network requests — the
    /// semaphore size. Default: [`default_network_concurrency`].
    pub network_concurrency: usize,

    /// Per-request total deadline, applied as both reqwest's response
    /// timeout and its connect timeout, bounding the whole request.
    /// Default: [`DEFAULT_FETCH_TIMEOUT_MS`].
    pub fetch_timeout: Duration,

    /// Value of the `User-Agent` header sent on every request.
    /// Default: [`DEFAULT_USER_AGENT`].
    pub user_agent: String,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        NetworkSettings {
            network_concurrency: default_network_concurrency(),
            fetch_timeout: Duration::from_millis(DEFAULT_FETCH_TIMEOUT_MS),
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
    client: Client,
    /// Per-registry clients keyed by nerf-darted URI. Empty when no
    /// `//host/:cert=…` / `:key=…` / `:ca=…` / `:cafile=…` /
    /// `:certfile=…` / `:keyfile=…` `.npmrc` entries are present —
    /// in which case `acquire_for_url` short-circuits to the default
    /// client without paying the routing cost.
    per_registry: HashMap<String, Client>,
    /// Pre-built routing table cloned from [`PerRegistryTls`] so the
    /// hot path can call `pick_for_url` without holding a reference
    /// to `PerRegistryTls` (which lives on `Config`). Empty when
    /// `per_registry` is empty.
    routing: PerRegistryTls,
    /// Per-origin socket cap (the `maxSockets` setting). `None` (the
    /// default) leaves the per-origin socket count bounded only by
    /// `semaphore`; see [`HostSocketLimit`].
    host_socket_limit: Option<HostSocketLimit>,
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
    _permit: Permit,
    /// The per-origin `maxSockets` permit, held for the same request lifetime
    /// as `_permit`. `None` when no `maxSockets` cap is configured or the URL
    /// had no parseable origin.
    _host_permit: Option<OwnedSemaphorePermit>,
    client: &'a Client,
}

impl Deref for ThrottledClientGuard<'_> {
    type Target = Client;

    fn deref(&self) -> &Client {
        self.client
    }
}

impl ThrottledClient {
    /// Acquire a permit and return a guard granting access to the
    /// underlying [`Client`]. The permit is released when the guard
    /// is dropped, so callers control how long the request "counts"
    /// against [`default_network_concurrency`] — typically the full
    /// `send + body-consume` lifetime, not just `.send()`.
    pub async fn acquire(&self) -> ThrottledClientGuard<'_> {
        let permit = self.semaphore.acquire(UNPRIORITIZED).await;
        ThrottledClientGuard { _permit: permit, _host_permit: None, client: &self.client }
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

    /// Construct the default throttled client used for real installs.
    ///
    /// Network topology (see [#280](https://github.com/pnpm/pacquet/issues/280)):
    ///
    /// * **HTTP/1.1 only.** A default `reqwest::Client` upgrades to
    ///   HTTP/2 via ALPN whenever the registry advertises it
    ///   (registry.npmjs.org does). HTTP/2 is deliberately disabled —
    ///   multiplexing many tarball streams over 1-2 TCP connections
    ///   sharing one congestion window was slower than opening ~50
    ///   independent HTTP/1.1 connections that each get their own
    ///   congestion window and saturate bandwidth in parallel.
    /// * **[`NetworkSettings::network_concurrency`] concurrent
    ///   in-flight requests**, defaulting to the `networkConcurrency`
    ///   formula (see [`default_network_concurrency`]). A 50-socket
    ///   per-host pool ceiling bounds total sockets, while a smaller
    ///   request-level cap bounds how many fetches actually run at once;
    ///   pacquet's semaphore plays the second role.
    /// * **A `User-Agent` header** ([`NetworkSettings::user_agent`],
    ///   defaulting to [`DEFAULT_USER_AGENT`]). A default
    ///   `reqwest::Client` sends no UA, which can trip CDN / WAF rules
    ///   that reject or RST bot-shaped traffic before any HTTP response
    ///   is produced.
    ///
    /// `pool_idle_timeout(4s)` matches
    /// [`agentkeepalive`'s](https://github.com/node-modules/agentkeepalive/blob/1e5e312f36/lib/agent.js#L39-L41)
    /// default `freeSocketTimeout` (the agent pnpm builds its
    /// connection pool on top of). Most CDN / load-balancer edges in
    /// front of `registry.npmjs.org` close idle sockets after 5–15s
    /// without sending FIN that hyper notices; a pool TTL above that
    /// lets pacquet reuse a half-dead socket and surface the next
    /// request as a generic "error sending request for url". 4s
    /// keeps the pool useful for back-to-back downloads (pacquet
    /// runs hundreds of fetches in seconds) but well below the
    /// typical edge keepalive.
    ///
    /// [`NetworkSettings::fetch_timeout`] is the per-request deadline,
    /// not the socket inactivity timeout. A default `reqwest::Client`
    /// has no deadlines at all, so a stalled upstream hangs the install
    /// indefinitely. It is applied as both the response timeout and the
    /// connect timeout, bounding the whole fetch. Default:
    /// [`DEFAULT_FETCH_TIMEOUT_MS`] (60s), the `fetchTimeout` setting's
    /// default.
    ///
    /// `hickory_dns(true)` swaps reqwest's default resolver
    /// (tokio's `lookup_host`, which calls the platform's blocking
    /// `getaddrinfo` from a `spawn_blocking` thread) for the
    /// pure-Rust async resolver. The default resolver is correct
    /// but on macOS it routes every lookup through `mDNSResponder`,
    /// which spuriously returns `EAI_NONAME` ("nodename nor servname
    /// provided") for valid hostnames when many concurrent lookups
    /// pile up — e.g. the [`default_network_concurrency`] simultaneous
    /// tarball connections this client opens. pnpm doesn't hit it
    /// because Node's `dns.lookup`
    /// runs on libuv's 4-thread pool, naturally throttling concurrent
    /// `getaddrinfo` calls. `hickory-dns` queries DNS over UDP / TCP
    /// directly, bypassing `mDNSResponder` and the `EAI_NONAME` flake
    /// entirely.
    #[must_use]
    pub fn new_for_installs() -> Self {
        Self::for_installs(
            &ProxyConfig::default(),
            &TlsConfig::default(),
            &PerRegistryTls::default(),
            &NetworkSettings::default(),
        )
        .expect("default proxy + TLS configs carry no URLs/PEMs and cannot fail")
    }

    /// Construct the install client with proxy + TLS configuration
    /// applied onto reqwest:
    /// * **Proxy routing.** HTTPS targets route through `https_proxy`,
    ///   HTTP targets through `http_proxy`, and [`ProxyConfig::no_proxy`]
    ///   short-circuits both via a per-URL custom-proxy closure.
    ///   Basic-auth user/password halves embedded in the proxy URL
    ///   are percent-decoded before being forwarded as the
    ///   `Proxy-Authorization` header.
    /// * **TLS.** Each PEM in [`TlsConfig::ca`] is added as a trusted
    ///   root via `reqwest::Certificate::from_pem`. When both
    ///   [`TlsConfig::cert`] and [`TlsConfig::key`] are set, they are
    ///   concatenated and passed to `Identity::from_pem` (rustls
    ///   single-buffer form). rustls accepts PKCS#1, PKCS#8, and EC
    ///   private keys — the same surface Node's `tls` exposes.
    ///   `strict_ssl` defaults to `true` and disables both
    ///   chain-of-trust and hostname verification when `false` — same
    ///   as Node's `rejectUnauthorized=false` short-circuit.
    /// * **`local_address`.** Pinned via
    ///   `reqwest::ClientBuilder::local_address`.
    ///
    /// Returns [`ProxyError::InvalidProxy`] when either configured
    /// proxy URL fails to parse even after the auto-`http://` prefix
    /// retry (the `ERR_PNPM_INVALID_PROXY` code), or [`TlsError`] when
    /// any CA or client identity PEM is malformed.
    /// pnpm does not define `ERR_PNPM_INVALID_CA` / similar codes —
    /// see [`TlsError`] for why pacquet still surfaces the failure
    /// eagerly rather than at request time.
    pub fn for_installs(
        proxy: &ProxyConfig,
        tls: &TlsConfig,
        per_registry: &PerRegistryTls,
        settings: &NetworkSettings,
    ) -> Result<Self, ForInstallsError> {
        Self::for_installs_with_redirect(proxy, tls, per_registry, settings, None)
    }

    /// Like [`Self::new_for_installs`] but installs `redirect_guard` as the
    /// client's redirect policy: every redirect hop is re-validated by the
    /// guard, and a hop it rejects fails the request without fetching. pnpr
    /// resolves on behalf of untrusted callers, so it passes a guard that
    /// re-checks each redirect target against its fetch allowlist — otherwise
    /// an allowlisted registry could `302` pnpr onto an internal host, slipping
    /// a server-side request past the request-boundary allowlist (SSRF). The
    /// CLI fetches on the user's own behalf and keeps the default follow
    /// policy via [`Self::new_for_installs`].
    #[must_use]
    pub fn new_for_installs_with_redirect_guard(
        is_allowed: impl Fn(&reqwest::Url) -> bool + Send + Sync + 'static,
    ) -> Self {
        let redirect_guard: RedirectGuard = Arc::new(is_allowed);
        Self::for_installs_with_redirect(
            &ProxyConfig::default(),
            &TlsConfig::default(),
            &PerRegistryTls::default(),
            &NetworkSettings::default(),
            Some(&redirect_guard),
        )
        .expect("default proxy + TLS configs carry no URLs/PEMs and cannot fail")
    }

    fn for_installs_with_redirect(
        proxy: &ProxyConfig,
        tls: &TlsConfig,
        per_registry: &PerRegistryTls,
        settings: &NetworkSettings,
        redirect_guard: Option<&RedirectGuard>,
    ) -> Result<Self, ForInstallsError> {
        if settings.network_concurrency == 0 {
            return Err(ForInstallsError::ZeroNetworkConcurrency);
        }
        let https = proxy.https_proxy.as_deref().map(parse_proxy_url).transpose()?;
        let http = proxy.http_proxy.as_deref().map(parse_proxy_url).transpose()?;
        let no_proxy = Arc::new(NoProxyMatcher::from(proxy.no_proxy.as_ref()));
        // Read once here, not inside `build_client`: `for_installs`
        // builds one client per per-registry override, so loading the
        // bundle per call would re-read and re-parse it N times.
        let extra_ca_certs = load_node_extra_ca_certs();

        let build_client = |effective_tls: &TlsConfig| -> Result<Client, ForInstallsError> {
            let mut builder = default_client_builder(settings);
            if let Some(url) = https.clone() {
                builder = builder.proxy(build_scheme_proxy(url, "https", Arc::clone(&no_proxy)));
            }
            if let Some(url) = http.clone() {
                builder = builder.proxy(build_scheme_proxy(url, "http", Arc::clone(&no_proxy)));
            }
            // Lowest-priority additive roots; `apply_tls` layers the
            // `.npmrc` ca/cafile roots on top next.
            for cert in &extra_ca_certs {
                builder = builder.add_root_certificate(cert.clone());
            }
            builder = apply_tls(builder, effective_tls)?;
            if let Some(guard) = redirect_guard {
                builder = builder.redirect(allowlist_redirect_policy(Arc::clone(guard)));
            }
            Ok(builder.build().expect("build reqwest client with default timeouts and proxy"))
        };

        let default_client = build_client(tls)?;
        // Build one client per per-registry override. Each gets a
        // merged `TlsConfig` where the per-registry fields shadow
        // their top-level counterparts field-by-field. `strict_ssl` and
        // `local_address` are top-level-only, so the per-registry client
        // still honors the top-level values.
        let mut per_registry_clients = HashMap::with_capacity(per_registry.iter().count());
        for (uri, override_) in per_registry.iter() {
            let merged = merge_tls(tls, override_);
            per_registry_clients.insert(uri.to_string(), build_client(&merged)?);
        }

        Ok(ThrottledClient {
            semaphore: PrioritySemaphore::new(settings.network_concurrency),
            client: default_client,
            per_registry: per_registry_clients,
            routing: per_registry.clone(),
            host_socket_limit: None,
        })
    }

    /// Construct a throttled client wrapping a pre-built [`Client`].
    /// Useful for tests that want different timeout values than
    /// [`Self::new_for_installs`] sets — e.g. sub-second connect
    /// timeouts so firewalled / unreachable URLs fail within the
    /// test-suite budget instead of waiting on TCP retry.
    #[must_use]
    pub fn from_client(client: Client) -> Self {
        let semaphore = PrioritySemaphore::new(default_network_concurrency());
        ThrottledClient {
            semaphore,
            client,
            per_registry: HashMap::new(),
            routing: PerRegistryTls::default(),
            host_socket_limit: None,
        }
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
    /// pool at an explicit `priority` instead of [`UNPRIORITIZED`] —
    /// the throughput class of the two-class grant policy. Tarball
    /// downloads pass their estimated pipeline work (0 when unknown)
    /// so that freed slots go to the most expensive pending archive
    /// first — the longest download+extract jobs start earliest and
    /// never end up running alone after the small ones drained.
    pub async fn acquire_for_url_with_priority(
        &self,
        url: &str,
        priority: u64,
    ) -> ThrottledClientGuard<'_> {
        // Acquire the per-origin `maxSockets` permit *before* the global
        // concurrency permit: a request queued behind a saturated origin must
        // not hold a global slot while it waits, or a burst to one origin would
        // hoard every global permit and starve requests to other origins.
        let host_permit = match &self.host_socket_limit {
            Some(limit) => limit.acquire(url).await,
            None => None,
        };
        let permit = self.semaphore.acquire(priority).await;
        let client = self
            .routing
            .pick_for_url(url)
            .and_then(|key| self.per_registry.get(key))
            .unwrap_or(&self.client);
        ThrottledClientGuard { _permit: permit, _host_permit: host_permit, client }
    }
}

/// Shared builder with the install-time defaults
/// ([`ThrottledClient::new_for_installs`] documents the why behind each
/// setting). Both `new_for_installs` and [`ThrottledClient::for_installs`]
/// route through this helper so a single source of truth governs
/// timeouts, HTTP-version, resolver, and the User-Agent header.
///
/// `settings.fetch_timeout` drives both the per-request response
/// timeout and the connect timeout, bounding the whole fetch.
/// `settings.user_agent` is sent verbatim; a value that cannot be
/// encoded as an HTTP header falls back to [`DEFAULT_USER_AGENT`].
/// A redirect-hop validator: returns `true` to follow a redirect to `url`,
/// `false` to block it. See
/// [`ThrottledClient::new_for_installs_with_redirect_guard`].
pub type RedirectGuard = Arc<dyn Fn(&reqwest::Url) -> bool + Send + Sync>;

/// Cap on redirect hops, matching reqwest's default `Policy::default()` limit
/// so the guarded client doesn't follow a redirect chain further than the
/// unguarded one would.
const MAX_REDIRECT_HOPS: usize = 10;

/// A redirect target the [`RedirectGuard`] rejected. Surfaced as the request
/// error so a blocked redirect fails loudly rather than silently fetching.
#[derive(Debug)]
struct BlockedRedirect(reqwest::Url);

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

/// A reqwest redirect policy that consults `guard` for every hop: an allowed
/// target is followed (up to [`MAX_REDIRECT_HOPS`]), a rejected one fails the
/// request with [`BlockedRedirect`] instead of being fetched.
fn allowlist_redirect_policy(guard: RedirectGuard) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        let target = attempt.url().clone();
        if attempt.previous().len() >= MAX_REDIRECT_HOPS || !guard(&target) {
            attempt.error(BlockedRedirect(target))
        } else {
            attempt.follow()
        }
    })
}

fn default_client_builder(settings: &NetworkSettings) -> reqwest::ClientBuilder {
    let user_agent = HeaderValue::from_str(&settings.user_agent)
        .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_USER_AGENT));
    let mut default_headers = HeaderMap::with_capacity(1);
    default_headers.insert(USER_AGENT, user_agent);
    Client::builder()
        .http1_only()
        // Request gzip and transparently decompress it. Packuments are the
        // largest payloads pulled during resolution and registries serve
        // them gzipped; tarballs are unaffected (no `Content-Encoding`, so
        // store-integrity verification still sees the raw `.tgz`). Defaults
        // to on with reqwest's `gzip` feature, but set explicitly so the
        // intent is visible and survives a change to that default.
        .gzip(true)
        .default_headers(default_headers)
        .connect_timeout(settings.fetch_timeout)
        .timeout(settings.fetch_timeout)
        .pool_idle_timeout(Duration::from_secs(4))
        .hickory_dns(true)
}

/// Load the PEM bundle named by `NODE_EXTRA_CA_CERTS` as extra trust
/// roots, to be added to every client `for_installs` builds.
///
/// `NODE_EXTRA_CA_CERTS` is the standard Node convention for appending
/// a CA to the default trust store. pnpm-on-Node inherits that trust
/// implicitly because it runs inside Node; pacquet is a native binary,
/// so to keep real-world parity for users behind a corporate MITM proxy
/// it reads the variable explicitly. This is the one deliberate
/// exception to the ".npmrc-only, no env vars" TLS parity policy
/// documented in [`tls::TlsConfig`]: the variable is a process-global
/// Node convention rather than a pnpm setting, and Node already honors
/// it for pnpm today — so reading it *restores* parity rather than
/// diverging from it. The certs are added in
/// [`ThrottledClient::for_installs`] (not [`apply_tls`]) so the
/// `.npmrc`-derived [`TlsConfig`] stays env-free.
///
/// Read and parsed once per [`ThrottledClient::for_installs`] call —
/// that constructor builds one client per per-registry override, so
/// loading here (rather than inside the per-client builder) avoids
/// re-reading and re-parsing the bundle N times during startup.
///
/// The resulting certs are additive and lowest-priority: layered under
/// the `.npmrc` `ca` / `cafile` roots that [`apply_tls`] adds afterward
/// and under the built-in webpki roots (ordering is immaterial — the
/// rustls root store is a union). A missing, unreadable, or malformed
/// file yields an empty list, matching pnpm's silent treatment of a
/// missing `cafile` rather than failing the client build.
fn load_node_extra_ca_certs() -> Vec<Certificate> {
    let Some(path) = std::env::var_os("NODE_EXTRA_CA_CERTS").filter(|value| !value.is_empty())
    else {
        return Vec::new();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return Vec::new();
    };
    Certificate::from_pem_bundle(&bytes).unwrap_or_default()
}

/// Apply [`TlsConfig`] onto a [`reqwest::ClientBuilder`]: register each
/// CA, install the client identity, set `danger_accept_invalid_certs`
/// when `strict_ssl: false`, and pin the outbound interface. Returns
/// the modified builder unchanged when every field is `None` / empty —
/// matching pnpm's "TLS-unset is default-TLS" semantics.
///
/// `strict_ssl` defaults to `true` here (`unwrap_or(true)`) rather than
/// in the config layer because that's where pnpm applies the same
/// default — see the "Defaults" section of [`TlsConfig`]. Failures from
/// PEM parsing surface as [`TlsError::InvalidCa`] /
/// [`TlsError::InvalidClientIdentity`] and bubble through
/// [`ForInstallsError`].
/// Build the effective [`TlsConfig`] for a per-registry override:
/// each scoped field (`ca`, `cert`, `key`) replaces its top-level
/// counterpart field-by-field; `strict_ssl` and `local_address`
/// always come from the top-level (only `:cert(file)?` / `:key(file)?`
/// / `:ca(file)?` are recognized as per-registry keys).
///
/// The `ca` field is special: a per-registry `ca` is stored as a
/// single string that may contain multiple concatenated PEMs, while
/// the top-level `ca` is a `Vec<String>` (the `cafile` loader split).
/// When the override has a `ca`, the effective top-level CA list is
/// *replaced* (not merged) by a one-element list with the scoped PEM
/// blob — which `Certificate::from_pem` handles fine since it accepts
/// multi-cert PEM buffers.
fn merge_tls(top: &TlsConfig, override_: &RegistryTls) -> TlsConfig {
    TlsConfig {
        ca: match &override_.ca {
            Some(pem) => vec![pem.clone()],
            None => top.ca.clone(),
        },
        cert: override_.cert.clone().or_else(|| top.cert.clone()),
        key: override_.key.clone().or_else(|| top.key.clone()),
        strict_ssl: top.strict_ssl,
        local_address: top.local_address,
    }
}

/// Lightweight syntactic check that `pem` contains at least one
/// `-----BEGIN CERTIFICATE-----` / `-----END CERTIFICATE-----` armor
/// pair. Catches the "user pasted garbage instead of PEM" case
/// without parsing the base64 body — rustls's
/// `Certificate::from_pem` stores the bytes verbatim and validates
/// lazily, so without this guard a malformed CA would silently slip
/// through and the install would proceed against an unknown trust
/// root. A stricter parse (base64 decode + DER validation) is left
/// to rustls itself when the connection is actually made.
fn looks_like_pem_cert(pem: &str) -> bool {
    let begin = pem.find("-----BEGIN CERTIFICATE-----");
    let end = pem.rfind("-----END CERTIFICATE-----");
    matches!((begin, end), (Some(b), Some(e)) if b < e)
}

fn apply_tls(
    mut builder: reqwest::ClientBuilder,
    tls: &TlsConfig,
) -> Result<reqwest::ClientBuilder, TlsError> {
    for (index, pem) in tls.ca.iter().enumerate() {
        // Validate the PEM armor *before* handing to reqwest.
        // Reqwest's rustls backend stores the bytes verbatim and
        // parses lazily at `Client::build()` time — a garbage CA
        // entry would otherwise be silently dropped and the install
        // would proceed against an unknown trust root. The eager
        // check catches the no-armor case (the common "user
        // pasted a path instead of PEM contents" failure) and lets
        // the malformed-CA error point at the specific entry in
        // the list.
        if !looks_like_pem_cert(pem) {
            return Err(TlsError::InvalidCa {
                index,
                reason: "missing `-----BEGIN CERTIFICATE-----` / `-----END CERTIFICATE-----` \
                         armor"
                    .to_string(),
            });
        }
        let cert = Certificate::from_pem(pem.as_bytes())
            .map_err(|source| TlsError::InvalidCa { index, reason: source.to_string() })?;
        builder = builder.add_root_certificate(cert);
    }
    if let (Some(cert), Some(key)) = (tls.cert.as_deref(), tls.key.as_deref()) {
        // reqwest's `Identity::from_pem` (gated on the `rustls`
        // feature pacquet builds with) takes a single PEM buffer
        // containing *both* the certificate and the private key, in
        // any order. Concatenating with a `\n` separator handles
        // both pnpm-style configs (where `cert=` and `key=` arrive
        // separately) and users who paste them into one field.
        //
        // rustls accepts PKCS#1 (`-----BEGIN RSA PRIVATE KEY-----`),
        // PKCS#8 (`-----BEGIN PRIVATE KEY-----`), and EC
        // (`-----BEGIN EC PRIVATE KEY-----`) private keys — same
        // surface area Node's `tls.createSecureContext` exposes,
        // and the surface pnpm hands to undici. PKCS#12 (`.pfx`) is
        // not supported by pnpm at the config layer (no `pfx=`
        // option in pnpm's `.npmrc` allow-list), so pacquet doesn't
        // need to handle it either.
        let combined = format!("{cert}\n{key}");
        let identity = Identity::from_pem(combined.as_bytes())
            .map_err(|source| TlsError::InvalidClientIdentity { reason: source.to_string() })?;
        builder = builder.identity(identity);
    }
    // The `strict-ssl` default is `true`, applied here at client-build
    // time rather than at config-parse time.
    if !tls.strict_ssl.unwrap_or(true) {
        builder = builder.danger_accept_invalid_certs(true);
    }
    if let Some(addr) = tls.local_address {
        builder = builder.local_address(addr);
    }
    Ok(builder)
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

/// Build a [`Proxy`] that routes only requests whose target scheme matches
/// `scheme` ("http" or "https") and whose host doesn't fall under the
/// no-proxy bypass. Userinfo is stripped from the URL and re-attached
/// via [`Proxy::basic_auth`] after percent-decoding so usernames /
/// passwords with `%XX` escapes (e.g. `@` in a password) reach the
/// upstream proxy decoded.
fn build_scheme_proxy(
    url: reqwest::Url,
    scheme: &'static str,
    no_proxy: Arc<NoProxyMatcher>,
) -> Proxy {
    let (clean_url, auth) = strip_userinfo(url);
    let mut proxy = Proxy::custom(move |target| {
        if no_proxy.matches_url(target) {
            return None;
        }
        (target.scheme() == scheme).then(|| clean_url.clone())
    });
    if let Some((user, pass)) = auth {
        proxy = proxy.basic_auth(&user, &pass);
    }
    proxy
}

/// Default number of concurrent in-flight network requests.
///
/// The `networkConcurrency` formula:
///
/// ```text
/// networkConcurrency = min(96, max(maxWorkers * 3, 64))
/// // maxWorkers = max(1, availableParallelism() - 1)
/// ```
///
/// Concretely: 64 up to a 22-core machine, scaling with cores beyond
/// that, capped at 96. The floor matters more than the scaling:
/// downloads are I/O-bound, not CPU-bound, and a low-latency registry
/// only saturates when enough requests are in flight — a CPU-derived
/// floor left 4-core CI runners draining 600-tarball installs 16 at a
/// time, several times slower than the same network could serve.
///
/// Uses [`std::thread::available_parallelism`] rather than
/// `num_cpus::get()` so cgroup / CPU-quota limits in containers and
/// CI runners are respected — `num_cpus` reports the host's logical
/// CPU count, which on a quota-limited runner can over-report and
/// push effective concurrency past what the kernel will actually
/// schedule (matching the convention `crates/cli` already uses for
/// rayon pool sizing, see `crates/cli/src/lib.rs`).
pub fn default_network_concurrency() -> usize {
    let available_parallelism = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    let max_workers = available_parallelism.saturating_sub(1).max(1);
    max_workers.saturating_mul(3).clamp(64, 96)
}

/// This is only necessary for tests.
impl Default for ThrottledClient {
    fn default() -> Self {
        ThrottledClient::new_for_installs()
    }
}
