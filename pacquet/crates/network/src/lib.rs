mod auth;
mod proxy;
#[cfg(test)]
mod tests;
mod tls;

pub use auth::{AuthHeaders, base64_encode, nerf_dart};
pub use proxy::{NoProxySetting, ProxyConfig, ProxyError};
pub use tls::{PerRegistryTls, RegistryTls, TlsConfig, TlsError};

use proxy::{NoProxyMatcher, parse_proxy_url, strip_userinfo};
use reqwest::{
    Certificate, Client, Identity, Proxy,
    header::{HeaderMap, HeaderValue, USER_AGENT},
};
use std::{collections::HashMap, num::NonZeroUsize, ops::Deref, sync::Arc, time::Duration};
use tokio::sync::{Semaphore, SemaphorePermit};

/// Default `User-Agent` pacquet sends on every request made by the
/// install client — registry metadata fetches and tarball downloads
/// alike, including tarball URLs that point at non-registry hosts.
///
/// Identical to pnpm v11's
/// [`network/fetch/src/fetchFromRegistry.ts`](https://github.com/pnpm/pnpm/blob/1819226b51/network/fetch/src/fetchFromRegistry.ts#L9):
/// the literal string `pnpm`. A default `reqwest::Client` sends *no*
/// User-Agent at all, which some registry CDNs and corporate WAFs
/// treat as a bot signature and either block at the edge or terminate
/// mid-handshake (surfacing as a generic "error sending request for
/// url" with no body to look at).
///
/// We deliberately send `pnpm` rather than `pacquet/<version>` so
/// any UA-keyed allow / rate-limit rule that lets pnpm through also
/// lets pacquet through. Pacquet is a port of pnpm; behavioural
/// parity, including what the registry sees on the wire, is the
/// goal.
const DEFAULT_USER_AGENT: &str = "pnpm";

/// Wrapper around [`Client`] with concurrent request limit enforced by the [`Semaphore`] mechanism.
///
/// Holds a default [`Client`] for the top-level proxy / TLS config
/// plus an optional map of per-registry clients keyed by nerf-darted
/// URI. [`Self::acquire_for_url`] picks the right client based on the
/// request URL (matching pnpm's [`pickSettingByUrl`](https://github.com/pnpm/pnpm/blob/94240bc046/network/fetch/src/dispatcher.ts#L338-L375)
/// 5-step fallback), and [`Self::acquire`] always uses the default
/// client. The semaphore is shared across both — bounding the total
/// concurrent socket count regardless of which registry a request
/// targets.
#[derive(Debug)]
pub struct ThrottledClient {
    semaphore: Semaphore,
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
    _permit: SemaphorePermit<'a>,
    client: &'a Client,
}

impl<'a> Deref for ThrottledClientGuard<'a> {
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
        let permit =
            self.semaphore.acquire().await.expect("semaphore shouldn't have been closed this soon");
        ThrottledClientGuard { _permit: permit, client: &self.client }
    }

    /// Construct the default throttled client used for real installs.
    ///
    /// Network topology is ported from pnpm v11's
    /// `network/fetch/src/dispatcher.ts` (see #280):
    ///
    /// * **HTTP/1.1 only.** A default `reqwest::Client` upgrades to
    ///   HTTP/2 via ALPN whenever the registry advertises it
    ///   (registry.npmjs.org does). Pnpm explicitly disables this
    ///   upstream after benchmarking — multiplexing many tarball
    ///   streams over 1-2 TCP connections sharing one congestion
    ///   window was slower than opening ~50 independent HTTP/1.1
    ///   connections that each get their own congestion window and
    ///   saturate bandwidth in parallel.
    /// * **`network_concurrency` concurrent in-flight requests**,
    ///   matching pnpm's `networkConcurrency` default (see
    ///   [`default_network_concurrency`]). Pnpm uses a 50-socket
    ///   per-host pool ceiling (`DEFAULT_MAX_SOCKETS` in
    ///   `network/fetch/src/dispatcher.ts`) *and* a smaller
    ///   request-level cap that bounds how many fetches it actually
    ///   runs at once; pacquet's semaphore plays the second role.
    /// * **`User-Agent: pnpm`** matching pnpm's
    ///   `fetchFromRegistry.ts`. A default `reqwest::Client` sends
    ///   no UA, which can trip CDN / WAF rules that reject or RST
    ///   bot-shaped traffic before any HTTP response is produced.
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
    /// `timeout(5min)` is the per-request deadline, not the socket
    /// inactivity timeout. A default `reqwest::Client` has no
    /// deadlines at all, so a stalled upstream hangs the install
    /// indefinitely. 5 min is deliberately generous — npm tarballs
    /// are usually under 5 MB but can reach hundreds of MB on slow
    /// connections — and catches truly stuck sockets, not
    /// short-lived hiccups.
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
    /// directly, bypassing `mDNSResponder` and the EAI_NONAME flake
    /// entirely.
    pub fn new_for_installs() -> Self {
        Self::for_installs(
            &ProxyConfig::default(),
            &TlsConfig::default(),
            &PerRegistryTls::default(),
        )
        .expect("default proxy + TLS configs carry no URLs/PEMs and cannot fail")
    }

    /// Construct the install client with proxy + TLS configuration
    /// applied.
    ///
    /// Ports pnpm v11's
    /// [`getDispatcher`](https://github.com/pnpm/pnpm/blob/94240bc046/network/fetch/src/dispatcher.ts#L23-L31)
    /// onto reqwest:
    /// * **Proxy routing.** HTTPS targets route through `https_proxy`,
    ///   HTTP targets through `http_proxy`, and [`ProxyConfig::no_proxy`]
    ///   short-circuits both via a per-URL custom-proxy closure.
    ///   Basic-auth user/password halves embedded in the proxy URL
    ///   are percent-decoded before being forwarded as the
    ///   `Proxy-Authorization` header — matching upstream's
    ///   [decode at dispatcher.ts:180-182](https://github.com/pnpm/pnpm/blob/94240bc046/network/fetch/src/dispatcher.ts#L180-L182).
    /// * **TLS.** Each PEM in [`TlsConfig::ca`] is added as a trusted
    ///   root via `reqwest::Certificate::from_pem`. When both
    ///   [`TlsConfig::cert`] and [`TlsConfig::key`] are set, they are
    ///   concatenated and passed to `Identity::from_pem` (rustls
    ///   single-buffer form). rustls accepts PKCS#1, PKCS#8, and EC
    ///   private keys — the same surface Node's `tls` exposes to
    ///   pnpm. `strict_ssl` defaults to `true` and
    ///   disables both chain-of-trust and hostname verification when
    ///   `false` — same as Node's `rejectUnauthorized=false`
    ///   short-circuit that pnpm forwards through undici
    ///   ([`dispatcher.ts:191,197,241,295`](https://github.com/pnpm/pnpm/blob/94240bc046/network/fetch/src/dispatcher.ts#L191)).
    /// * **`local_address`.** Pinned via
    ///   `reqwest::ClientBuilder::local_address`.
    ///
    /// Returns [`ProxyError::InvalidProxy`] when either configured
    /// proxy URL fails to parse even after the auto-`http://` prefix
    /// retry (matching upstream's `ERR_PNPM_INVALID_PROXY`), or
    /// [`TlsError`] when any CA or client identity PEM is malformed.
    /// pnpm does not define `ERR_PNPM_INVALID_CA` / similar codes —
    /// see [`TlsError`] for why pacquet still surfaces the failure
    /// eagerly rather than at request time.
    pub fn for_installs(
        proxy: &ProxyConfig,
        tls: &TlsConfig,
        per_registry: &PerRegistryTls,
    ) -> Result<Self, ForInstallsError> {
        let https = proxy.https_proxy.as_deref().map(parse_proxy_url).transpose()?;
        let http = proxy.http_proxy.as_deref().map(parse_proxy_url).transpose()?;
        let no_proxy = Arc::new(NoProxyMatcher::from(proxy.no_proxy.as_ref()));

        let build_client = |effective_tls: &TlsConfig| -> Result<Client, ForInstallsError> {
            let mut builder = default_client_builder();
            if let Some(url) = https.clone() {
                builder = builder.proxy(build_scheme_proxy(url, "https", Arc::clone(&no_proxy)));
            }
            if let Some(url) = http.clone() {
                builder = builder.proxy(build_scheme_proxy(url, "http", Arc::clone(&no_proxy)));
            }
            builder = apply_tls(builder, effective_tls)?;
            Ok(builder.build().expect("build reqwest client with default timeouts and proxy"))
        };

        let default_client = build_client(tls)?;
        // Build one client per per-registry override. Each gets a
        // merged `TlsConfig` where the per-registry fields shadow
        // their top-level counterparts (matching pnpm's
        // `{ ...opts, ...sslConfig }` spread at
        // [`dispatcher.ts:143,264`](https://github.com/pnpm/pnpm/blob/94240bc046/network/fetch/src/dispatcher.ts#L143)).
        // `strict_ssl` and `local_address` are top-level-only, so the
        // per-registry client still honors the top-level values.
        let mut per_registry_clients = HashMap::with_capacity(per_registry.iter().count());
        for (uri, override_) in per_registry.iter() {
            let merged = merge_tls(tls, override_);
            per_registry_clients.insert(uri.to_string(), build_client(&merged)?);
        }

        Ok(ThrottledClient {
            semaphore: Semaphore::new(default_network_concurrency()),
            client: default_client,
            per_registry: per_registry_clients,
            routing: per_registry.clone(),
        })
    }

    /// Construct a throttled client wrapping a pre-built [`Client`].
    /// Useful for tests that want different timeout values than
    /// [`Self::new_for_installs`] sets — e.g. sub-second connect
    /// timeouts so firewalled / unreachable URLs fail within the
    /// test-suite budget instead of waiting on TCP retry.
    pub fn from_client(client: Client) -> Self {
        let semaphore = Semaphore::new(default_network_concurrency());
        ThrottledClient {
            semaphore,
            client,
            per_registry: HashMap::new(),
            routing: PerRegistryTls::default(),
        }
    }

    /// Acquire a permit and return a guard granting access to the
    /// per-registry [`Client`] that matches `url`'s nerf-darted form
    /// (falling back to the default client when no override matches).
    /// The semaphore is shared across all clients, so total concurrent
    /// socket count stays bounded by [`default_network_concurrency`]
    /// regardless of which registry the request targets.
    ///
    /// Per-URL routing mirrors pnpm's
    /// [`pickSettingByUrl`](https://github.com/pnpm/pnpm/blob/94240bc046/network/fetch/src/dispatcher.ts#L338-L375)
    /// 5-step fallback: exact, then nerf-darted, then host without
    /// port, then progressively shorter path prefixes, then a
    /// recursive retry without port. When no per-registry overrides
    /// are configured (the common case), the routing table is empty
    /// and the lookup short-circuits to the default client.
    ///
    /// Takes `url` as `&str` so callers don't have to round-trip
    /// `format!("{registry}{name}")` strings through `Url::parse`
    /// just to satisfy the type signature — the lookup itself works
    /// on the raw string form.
    pub async fn acquire_for_url(&self, url: &str) -> ThrottledClientGuard<'_> {
        let permit =
            self.semaphore.acquire().await.expect("semaphore shouldn't have been closed this soon");
        let client = self
            .routing
            .pick_for_url(url)
            .and_then(|key| self.per_registry.get(key))
            .unwrap_or(&self.client);
        ThrottledClientGuard { _permit: permit, client }
    }
}

/// Shared builder with the install-time defaults
/// ([`ThrottledClient::new_for_installs`] documents the why behind each
/// setting). Both `new_for_installs` and [`ThrottledClient::for_installs`]
/// route through this helper so a single source of truth governs
/// timeouts, HTTP-version, resolver, and the default User-Agent header.
fn default_client_builder() -> reqwest::ClientBuilder {
    let mut default_headers = HeaderMap::with_capacity(1);
    default_headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
    Client::builder()
        .http1_only()
        .default_headers(default_headers)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(300))
        .pool_idle_timeout(Duration::from_secs(4))
        .hickory_dns(true)
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
/// always come from the top-level (pnpm doesn't honor scoped versions
/// of those keys — see [`getNetworkConfigs.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/getNetworkConfigs.ts#L94)
/// which only recognizes `:cert(file)?` / `:key(file)?` / `:ca(file)?`).
///
/// The `ca` field is special: pnpm stores per-registry `ca` as a
/// single string (`getNetworkConfigs.ts:37`) that may contain multiple
/// concatenated PEMs, while the top-level `ca` is a `Vec<String>`
/// (the `cafile` loader split). When the override has a `ca`, the
/// effective top-level CA list is *replaced* (per pnpm's spread, not
/// merged) by a one-element list with the scoped PEM blob — which
/// `Certificate::from_pem` handles fine since it accepts multi-cert
/// PEM buffers.
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
    // pnpm's `strict-ssl` default is `true`, applied at every
    // dispatcher emit site rather than at parse time.
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
/// upstream proxy decoded — matching pnpm's behavior at
/// [`dispatcher.ts:180-182`](https://github.com/pnpm/pnpm/blob/94240bc046/network/fetch/src/dispatcher.ts#L180-L182).
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
/// Mirrors pnpm's `networkConcurrency` formula in
/// [`installing/package-requester/src/packageRequester.ts`](https://github.com/pnpm/pnpm/blob/1819226b51/installing/package-requester/src/packageRequester.ts#L97)
/// and `calcMaxWorkers` in
/// [`worker/src/index.ts`](https://github.com/pnpm/pnpm/blob/1819226b51/worker/src/index.ts#L63-L72):
///
/// ```ts
/// networkConcurrency = Math.min(64, Math.max(calcMaxWorkers() * 3, 16))
/// // calcMaxWorkers() = Math.max(1, availableParallelism() - 1)
/// ```
///
/// Concretely: 16 on a 4-core machine, 21 on 8-core, 27 on 10-core,
/// 45 on 16-core, capped at 64.
///
/// Uses [`std::thread::available_parallelism`] rather than
/// `num_cpus::get()` so cgroup / CPU-quota limits in containers and
/// CI runners are respected — `num_cpus` reports the host's logical
/// CPU count, which on a quota-limited runner can over-report and
/// push effective concurrency past what the kernel will actually
/// schedule (matching the convention `crates/cli` already uses for
/// rayon pool sizing, see `crates/cli/src/lib.rs`).
pub fn default_network_concurrency() -> usize {
    let available_parallelism =
        std::thread::available_parallelism().map(NonZeroUsize::get).unwrap_or(1);
    let max_workers = available_parallelism.saturating_sub(1).max(1);
    max_workers.saturating_mul(3).clamp(16, 64)
}

/// This is only necessary for tests.
impl Default for ThrottledClient {
    fn default() -> Self {
        ThrottledClient::new_for_installs()
    }
}
