use super::{
    Addrs, Arc, Client, DEFAULT_USER_AGENT, Duration, ForInstallsError, HeaderMap, HeaderValue,
    LazyLock, Name, NetworkSettings, NoProxyMatcher, NonZeroUsize, Proxy, Resolve, Resolving,
    Semaphore, TlsConfig, TrustRoots, USER_AGENT, apply_tls, bundled_root_certs, parse_proxy_url,
    strip_userinfo,
};

/// Shared builder with the install-time defaults
/// ([`ThrottledClient::new_for_installs`](crate::ThrottledClient::new_for_installs) documents the why behind each
/// setting). Both `new_for_installs` and [`ThrottledClient::for_installs`](crate::ThrottledClient::for_installs)
/// route through this helper so a single source of truth governs
/// timeouts, HTTP-version, resolver, and the User-Agent header.
///
/// `settings.fetch_timeout` drives both the read timeout and the
/// connect timeout, bounding how long a request may make no progress.
/// `settings.user_agent` is sent verbatim; a value that cannot be
/// encoded as an HTTP header falls back to [`DEFAULT_USER_AGENT`].
/// A redirect-hop validator: returns `true` to follow a redirect to `url`,
/// `false` to block it. See
/// [`ThrottledClient::new_for_installs_with_redirect_guard`](crate::ThrottledClient::new_for_installs_with_redirect_guard).
pub type RedirectGuard = Arc<dyn Fn(&reqwest::Url) -> bool + Send + Sync>;

/// Cap on redirect hops, matching reqwest's default `Policy::default()` limit
/// so the guarded client doesn't follow a redirect chain further than the
/// unguarded one would.
pub(super) const MAX_REDIRECT_HOPS: usize = 10;

/// A redirect target the [`RedirectGuard`] rejected. Surfaced as the request
/// error so a blocked redirect fails loudly rather than silently fetching.
#[derive(Debug)]
pub(super) struct BlockedRedirect(pub(super) reqwest::Url);

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

/// Validate every redirect before either following it or returning it to a
/// manual redirect loop. Rejected targets fail with [`BlockedRedirect`].
/// Everything a client build shares across the per-registry variants.
pub(super) struct ClientBuildInputs<'a> {
    pub(super) settings: &'a NetworkSettings,
    pub(super) https: Option<reqwest::Url>,
    pub(super) http: Option<reqwest::Url>,
    pub(super) no_proxy: Arc<NoProxyMatcher>,
    pub(super) extra_ca_certs: Vec<reqwest::Certificate>,
    pub(super) redirect_guard: Option<&'a RedirectGuard>,
}

/// Build one client, falling back to the bundled roots when the platform
/// trust store cannot be loaded.
pub(super) fn build_client_with_root_fallback(
    inputs: &ClientBuildInputs<'_>,
    effective_tls: &TlsConfig,
    forbid_redirects: bool,
) -> Result<Client, ForInstallsError> {
    let platform = match client_builder(
        inputs,
        effective_tls,
        TrustRoots::Platform,
        forbid_redirects,
    )?
    .build()
    {
        Ok(client) => return Ok(client),
        Err(platform) => platform,
    };
    client_builder(inputs, effective_tls, TrustRoots::Bundled, forbid_redirects)?
        .build()
        .map_err(|bundled| ForInstallsError::ClientBuild { platform, bundled })
}

/// The builder for one client: proxies, additive roots, TLS, and the redirect
/// policy.
fn client_builder(
    inputs: &ClientBuildInputs<'_>,
    effective_tls: &TlsConfig,
    trust_roots: TrustRoots,
    forbid_redirects: bool,
) -> Result<reqwest::ClientBuilder, ForInstallsError> {
    let mut builder = default_client_builder(inputs.settings);
    if let Some(url) = inputs.https.clone() {
        builder = builder.proxy(build_scheme_proxy(url, "https", Arc::clone(&inputs.no_proxy)));
    }
    if let Some(url) = inputs.http.clone() {
        builder = builder.proxy(build_scheme_proxy(url, "http", Arc::clone(&inputs.no_proxy)));
    }
    // Lowest-priority additive roots; `apply_tls` layers the `.npmrc`
    // ca/cafile roots on top next.
    for cert in &inputs.extra_ca_certs {
        builder = builder.add_root_certificate(cert.clone());
    }
    builder = apply_tls(builder, effective_tls)?;
    // Android's platform verifier requires a JVM, which the standalone CLI does not have.
    if cfg!(target_os = "android") || trust_roots == TrustRoots::Bundled {
        builder = builder.tls_certs_only(bundled_root_certs().iter().cloned());
    }
    Ok(apply_redirect_policy(builder, inputs.redirect_guard, forbid_redirects))
}

/// The proxy URL a setting names, treating an empty value as unset. See the
/// empty-value contract on [`ProxyConfig`](crate::proxy::ProxyConfig).
pub(super) fn configured_proxy(
    raw: Option<&str>,
) -> Result<Option<reqwest::Url>, ForInstallsError> {
    Ok(raw.filter(|value| !value.is_empty()).map(parse_proxy_url).transpose()?)
}

/// Apply the redirect policy: an allowlist guard when one is wired up, else
/// either reqwest's default or no redirects at all.
fn apply_redirect_policy(
    builder: reqwest::ClientBuilder,
    redirect_guard: Option<&RedirectGuard>,
    forbid_redirects: bool,
) -> reqwest::ClientBuilder {
    if let Some(guard) = redirect_guard {
        return builder.redirect(allowlist_redirect_policy(Arc::clone(guard), !forbid_redirects));
    }
    if forbid_redirects {
        return builder.redirect(reqwest::redirect::Policy::none());
    }
    builder
}

fn allowlist_redirect_policy(
    guard: RedirectGuard,
    follow_redirects: bool,
) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        let target = attempt.url().clone();
        if attempt.previous().len() >= MAX_REDIRECT_HOPS || !guard(&target) {
            attempt.error(BlockedRedirect(target))
        } else if follow_redirects {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

pub(super) fn is_redirect_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
}

/// Caps concurrent lookups through `Inner`. Clones share the cap.
#[derive(Clone)]
pub(super) struct CappedDnsResolver<Inner> {
    pub(super) inner: Arc<Inner>,
    pub(super) permits: Arc<Semaphore>,
}

#[derive(Clone)]
struct NativeDnsResolver;

impl Resolve for NativeDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            tokio::net::lookup_host((host, 0))
                .await
                .map(|addrs| Box::new(addrs) as Addrs)
                .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
        })
    }
}

/// Resolve through the platform's `getaddrinfo`, capped at Node's libuv
/// thread-pool width so `mDNSResponder` is not overloaded.
///
/// The system resolver is the only one that sees the whole host
/// configuration, which is why Hickory's pure-Rust resolver is not used
/// on any platform. macOS needs it for the scoped/supplemental resolver
/// graph (VPN split DNS), which Hickory does not read. Windows needs it
/// because Hickory sends every query from a freshly bound wildcard UDP
/// socket, and Windows Defender Firewall treats that bind as a listener:
/// it prompts the user to allow `pnpm.exe`, keyed on the executable's
/// path, so every newly installed engine prompts again
/// (pnpm/pnpm#14405). `getaddrinfo` hands the query to the Dnscache
/// service and binds nothing in this process, and it also honors NRPT and
/// per-adapter DNS settings. Linux needs it because Hickory parses
/// `/etc/resolv.conf` itself and rejects the whole file over one option
/// spelling glibc accepts (`no_tld_query`) or does not know yet, after
/// which reqwest silently falls back to Google's public nameservers
/// (pnpm/pnpm#14469). `getaddrinfo` also consults `nsswitch.conf`
/// sources such as `nss-resolve` and `nss-mdns` that Hickory bypasses.
pub(super) fn configure_dns(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    builder.dns_resolver(native_dns_resolver())
}

#[must_use]
pub fn native_dns_resolver() -> Arc<dyn Resolve> {
    static RESOLVER: LazyLock<Arc<CappedDnsResolver<NativeDnsResolver>>> = LazyLock::new(|| {
        const DNS_CONCURRENCY: NonZeroUsize = NonZeroUsize::new(4).expect("four is non-zero");
        Arc::new(CappedDnsResolver::new(NativeDnsResolver, DNS_CONCURRENCY))
    });
    Arc::clone(&RESOLVER) as Arc<dyn Resolve>
}

fn default_client_builder(settings: &NetworkSettings) -> reqwest::ClientBuilder {
    let user_agent = HeaderValue::from_str(&settings.user_agent)
        .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_USER_AGENT));
    let mut default_headers = HeaderMap::with_capacity(1);
    default_headers.insert(USER_AGENT, user_agent);
    let builder = Client::builder()
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
        .read_timeout(settings.fetch_timeout)
        .pool_idle_timeout(Duration::from_secs(4));
    configure_dns(builder)
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
