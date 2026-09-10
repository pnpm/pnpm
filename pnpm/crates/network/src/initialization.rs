use super::{
    Arc, Client, ClientBuildInputs, ClientPair, DEFAULT_FETCH_MIN_SPEED_KI_BPS,
    DEFAULT_FETCH_WARN_TIMEOUT_MS, Duration, ForInstallsError, NetworkSettings, NoProxyMatcher,
    PerRegistryTls, PrioritySemaphore, ProxyConfig, RedirectGuard, ThrottledClient, TlsConfig,
    build_client_with_root_fallback, configured_proxy, default_network_concurrency, ignore_warning,
    load_node_extra_ca_certs, merge_tls, tls,
};

impl ThrottledClient {
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
    ///   defaulting to [`DEFAULT_USER_AGENT`](crate::DEFAULT_USER_AGENT)). A default
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
    /// [`NetworkSettings::fetch_timeout`] bounds how long a request may
    /// make no progress, not how long it may run. A default
    /// `reqwest::Client` has no deadlines at all, so a stalled upstream
    /// hangs the install indefinitely. It is applied as reqwest's read
    /// timeout — which restarts on every chunk received — and as the
    /// connect timeout. A total deadline would abort a healthy download
    /// of a large archive over a slow link
    /// ([#14604](https://github.com/pnpm/pnpm/issues/14604)). Default:
    /// [`DEFAULT_FETCH_TIMEOUT_MS`](crate::DEFAULT_FETCH_TIMEOUT_MS) (60s), the `fetchTimeout` setting's
    /// default.
    ///
    /// Hostnames resolve through the platform's `getaddrinfo` behind a
    /// process-wide four-lookup cap shared by every client, matching
    /// Node's libuv DNS pool. `configure_dns` documents why no pure-Rust
    /// resolver is used on any platform.
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
    /// * **TLS.** Every certificate read out of [`TlsConfig::ca`] is
    ///   added as a trusted root; material the TLS backend cannot read
    ///   is skipped. When both [`TlsConfig::cert`] and
    ///   [`TlsConfig::key`] are set and neither is blank, they are
    ///   concatenated and passed to `Identity::from_pem` (rustls
    ///   single-buffer form). rustls accepts PKCS#1, PKCS#8, and EC
    ///   private keys — the same surface Node's `tls` exposes.
    ///   `strict_ssl` defaults to `true` and disables both
    ///   chain-of-trust and hostname verification when `false` — same
    ///   as Node's `rejectUnauthorized=false` short-circuit.
    /// * **`local_address`.** Pinned via
    ///   `reqwest::ClientBuilder::local_address`.
    /// * **Trust store.** The platform's, falling back to the Mozilla
    ///   roots bundled into the binary when the platform verifier
    ///   cannot be constructed (a system with no trust store at all).
    ///   Android always uses the bundled roots because the CLI has no JVM.
    ///
    /// Returns [`ProxyError::InvalidProxy`](crate::proxy::ProxyError::InvalidProxy) when either configured
    /// proxy URL fails to parse even after the auto-`http://` prefix
    /// retry (the `ERR_PNPM_INVALID_PROXY` code), or [`TlsError`](crate::tls::TlsError) when
    /// the client identity PEM is malformed. Unreadable CA material is
    /// not an error — see [`TlsError`](crate::tls::TlsError) for where that line sits.
    pub fn for_installs(
        proxy: &ProxyConfig,
        tls: &TlsConfig,
        per_registry: &PerRegistryTls,
        settings: &NetworkSettings,
    ) -> Result<Self, ForInstallsError> {
        Self::for_installs_with_redirect(proxy, tls, per_registry, settings, None)
    }

    /// Like [`Self::for_installs`] with an optional redirect guard.
    /// Pass `Some(guard)` to restrict which redirect targets are followed;
    /// `None` gives the default reqwest follow policy (same as [`Self::for_installs`]).
    pub fn for_installs_with_guard(
        proxy: &ProxyConfig,
        tls: &TlsConfig,
        per_registry: &PerRegistryTls,
        settings: &NetworkSettings,
        redirect_guard: Option<&RedirectGuard>,
    ) -> Result<Self, ForInstallsError> {
        Self::for_installs_with_redirect(proxy, tls, per_registry, settings, redirect_guard)
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

    pub(super) fn for_installs_with_redirect(
        proxy: &ProxyConfig,
        tls: &TlsConfig,
        per_registry: &PerRegistryTls,
        settings: &NetworkSettings,
        redirect_guard: Option<&RedirectGuard>,
    ) -> Result<Self, ForInstallsError> {
        if settings.network_concurrency == 0 {
            return Err(ForInstallsError::ZeroNetworkConcurrency);
        }
        // See the empty-value contract on `ProxyConfig`.
        let https = configured_proxy(proxy.https_proxy.as_deref())?;
        let http = configured_proxy(proxy.http_proxy.as_deref())?;
        let no_proxy = Arc::new(NoProxyMatcher::from(proxy.no_proxy.as_ref()));
        // Read once here, not inside `build_client`: `for_installs`
        // builds one client per per-registry override, so loading the
        // bundle per call would re-read and re-parse it N times.
        let extra_ca_certs = load_node_extra_ca_certs();

        let inputs =
            ClientBuildInputs { settings, https, http, no_proxy, extra_ca_certs, redirect_guard };
        let build_client = |effective_tls: &TlsConfig, forbid_redirects: bool| {
            build_client_with_root_fallback(&inputs, effective_tls, forbid_redirects)
        };

        let default_clients = ClientPair {
            follow_redirects: build_client(tls, false)?,
            no_redirects: build_client(tls, true)?,
        };
        // Build one client per per-registry override. Each gets a
        // merged `TlsConfig` where the per-registry fields shadow
        // their top-level counterparts field-by-field. `strict_ssl` and
        // `local_address` are top-level-only, so the per-registry client
        // still honors the top-level values.
        let per_registry = per_registry.try_map(|override_| -> Result<_, ForInstallsError> {
            let merged = merge_tls(tls, override_);
            Ok(ClientPair {
                follow_redirects: build_client(&merged, false)?,
                no_redirects: build_client(&merged, true)?,
            })
        })?;

        Ok(Self::from_client_pairs(default_clients, per_registry, settings))
    }

    /// Assemble the client around its built pairs and the settings every
    /// request is throttled by.
    pub(super) fn from_client_pairs(
        default_clients: ClientPair,
        per_registry: tls::PerRegistryMap<ClientPair>,
        settings: &NetworkSettings,
    ) -> Self {
        ThrottledClient {
            semaphore: PrioritySemaphore::new(settings.network_concurrency),
            default_clients,
            per_registry,
            host_socket_limit: None,
            fetch_warn_timeout: settings.fetch_warn_timeout,
            fetch_min_speed_ki_bps: settings.fetch_min_speed_ki_bps,
            warning_handler: std::sync::RwLock::new(ignore_warning),
        }
    }

    /// Construct a throttled client wrapping aligned pre-built clients.
    /// `client_without_redirects` must carry the same TLS, proxy, timeout,
    /// headers, and protocol settings as `client`, differing only in its
    /// redirect policy.
    #[must_use]
    pub fn from_clients(client: Client, client_without_redirects: Client) -> Self {
        let semaphore = PrioritySemaphore::new(default_network_concurrency());
        ThrottledClient {
            semaphore,
            default_clients: ClientPair {
                follow_redirects: client,
                no_redirects: client_without_redirects,
            },
            per_registry: tls::PerRegistryMap::default(),
            host_socket_limit: None,
            fetch_warn_timeout: Duration::from_millis(DEFAULT_FETCH_WARN_TIMEOUT_MS),
            fetch_min_speed_ki_bps: DEFAULT_FETCH_MIN_SPEED_KI_BPS,
            warning_handler: std::sync::RwLock::new(ignore_warning),
        }
    }
}
