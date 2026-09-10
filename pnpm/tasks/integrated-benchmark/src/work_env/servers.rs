use super::{
    BenchId, PNPR_SERVER_REGISTRY_ENV, PNPR_TARBALL_REWRITE_FROM_ENV, PnprServer, PnprServerPaths,
    RevisionMockRegistry, WorkEnv, append_pnpr_auth_to_npmrc, cold_mock_config_yaml,
    distinct_public_route_registries, mint_pnpr_token, seed_pnpr_auth, wait_for_pnpr_ready,
    write_pnpr_benchmark_config,
};
use crate::{
    cli_args::{BenchmarkScenario, RegistryMode, TargetKind},
    latency_proxy::{LatencyProxy, LinkProfile, mbps_to_bytes_per_sec},
};
use pnpm_registry_mock::pick_unused_port;
use std::{
    collections::HashMap,
    fs::{self, File},
    net::{Ipv4Addr, SocketAddr, TcpListener},
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

impl WorkEnv {
    /// Apply the `serve_timing` diagnostic env to a spawned `pnpr` command, if
    /// enabled — so the server emits per-phase serve timing to its log.
    fn apply_serve_timing(&self, command: &mut Command) {
        if self.serve_timing {
            // Append rather than clobber, so a `RUST_LOG` the caller already set
            // (to surface other diagnostics) keeps working alongside the timing
            // target. EnvFilter reads the last matching directive, and ours is
            // target-scoped, so appending never suppresses an existing filter.
            let directive = match std::env::var("RUST_LOG") {
                Ok(existing) if !existing.is_empty() => {
                    format!("{existing},pnpr::serve_timing=debug")
                }
                _ => "pnpr::serve_timing=debug".to_string(),
            };
            command.env("RUST_LOG", directive);
        }
    }
}
impl WorkEnv {
    /// Spawn the tarball-serving mock for every planned revision (see
    /// [`Self::plan_revision_mocks`]), each running that revision's `pnpr`
    /// binary in proxy mode against the shared warm runtime storage, fronted
    /// by the same client↔registry latency + bandwidth link the targets'
    /// `.npmrc` points at. The guards stop the mocks (and proxies) on drop;
    /// the vec is empty when no revision has its own mock.
    pub(super) fn start_revision_mocks(
        &self,
        revision_mocks: HashMap<String, RevisionMockRegistry>,
    ) -> Vec<PnprServer> {
        revision_mocks
            .into_iter()
            .map(|(revision, registry)| self.start_revision_mock(&revision, registry))
            .collect()
    }
    pub(super) fn start_revision_mock(
        &self,
        revision: &str,
        registry: RevisionMockRegistry,
    ) -> PnprServer {
        let binary = self.pnpr_server_binary(revision);
        assert!(
            binary.is_file(),
            "pnpr binary not found at {binary:?} — the build step did not produce it",
        );
        let mock_port = pick_unused_port().expect("pick a port for the revision mock");

        let bench_dir = self.bench_dir(BenchId::PnprRevision(revision));
        let stdout = File::create(bench_dir.join("revision-mock.stdout.log"))
            .expect("create revision mock stdout log");
        let stderr = File::create(bench_dir.join("revision-mock.stderr.log"))
            .expect("create revision mock stderr log");

        // The mock advertises its tarball URLs at the client-facing proxy
        // URL (`registry.url`), not its own loopback port, so downloads cross
        // the emulated registry link instead of bypassing it.
        let cold = self.scenario.is_some_and(BenchmarkScenario::cold_pnpr_cache);
        let mut command = if cold {
            self.cold_revision_mock_command(revision, &binary, &bench_dir, mock_port, &registry.url)
        } else {
            eprintln!(
                "Serving {revision}'s tarballs from a mock built from pnpr@{revision} on 127.0.0.1:{mock_port}...",
            );
            pnpm_registry_mock::pnpr_command_with_binary(&binary, mock_port, Some(&registry.url))
        };
        self.apply_serve_timing(&mut command);
        let process = command
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .expect(if cold { "spawn cold revision mock" } else { "spawn revision mock" });

        let mut server = PnprServer { process, latency_proxy: None };
        wait_for_pnpr_ready(mock_port);
        server.latency_proxy =
            Some(self.front_revision_mock(revision, registry.listener, mock_port));
        server
    }
    /// Use isolated storage, wiped between iterations, with the warm mock as origin.
    fn cold_revision_mock_command(
        &self,
        revision: &str,
        binary: &Path,
        bench_dir: &Path,
        mock_port: u16,
        public_url: &str,
    ) -> Command {
        let cold_storage = bench_dir.join("cold-mock-storage");
        let config_path = bench_dir.join("cold-mock-config.yaml");
        fs::write(&config_path, cold_mock_config_yaml(&cold_storage, &self.registry))
            .expect("write cold mock config");
        eprintln!(
            "Serving {revision}'s tarballs from a COLD mock built from pnpr@{revision} on 127.0.0.1:{mock_port} (origin {})...",
            self.registry,
        );
        let mut command = Command::new(binary);
        command
            .arg("--config")
            .arg(&config_path)
            .arg("--storage")
            .arg(&cold_storage)
            .arg("--listen")
            .arg(format!("127.0.0.1:{mock_port}"))
            .arg("--public-url")
            .arg(public_url)
            .arg("--packument-ttl-secs")
            .arg("31536000");
        command
    }
    /// Start a pnpr resolver server for every `pnpr@<rev>`
    /// target and write the `.pnpr-env` its `install.bash` sources. Each
    /// server gets an isolated `<bench_dir>/pnpr-storage`. The returned
    /// guards keep the servers alive and kill them on drop; the vec is
    /// empty when no target is a pnpr target.
    pub(super) fn start_pnpr_servers(&self, pnpr_server_registry: &str) -> Vec<PnprServer> {
        self.benchmarked_ids()
            .filter(|id| id.is_pnpr())
            .map(|id| self.start_pnpr_server(id, pnpr_server_registry))
            .collect()
    }
    /// Front the mock with the same latency + bandwidth profile the shared
    /// registry proxy uses, serving the socket reserved at planning time
    /// (and baked into this revision's `.npmrc`) so the port can't have
    /// been stolen during the build.
    fn front_revision_mock(
        &self,
        revision: &str,
        listener: TcpListener,
        mock_port: u16,
    ) -> LatencyProxy {
        let upstream = SocketAddr::from((Ipv4Addr::LOCALHOST, mock_port));
        let profile = LinkProfile {
            one_way: Duration::from_millis(self.registry_latency_ms) / 2,
            rate_limit: mbps_to_bytes_per_sec(self.registry_bandwidth_mbps),
            slow_start: self.registry_slow_start,
        };
        let proxy = LatencyProxy::spawn_with_listener(listener, upstream, profile)
            .expect("spawn revision mock latency proxy");
        eprintln!(
            "Fronting mock for {revision} with {}ms round-trip latency + {} download cap (proxy at {})",
            self.registry_latency_ms,
            match self.registry_bandwidth_mbps {
                mbps if mbps > 0.0 => format!("{mbps} Mbit/s"),
                _ => "no".to_string(),
            },
            proxy.addr,
        );
        proxy
    }
    pub(super) fn start_pnpr_server(&self, id: BenchId, pnpr_server_registry: &str) -> PnprServer {
        let bench_dir = self.bench_dir(id);
        let binary = bench_dir.join("pacquet").join("target").join("release").join("pnpr");
        assert!(
            binary.is_file(),
            "pnpr binary not found at {binary:?} — the build step did not produce it",
        );
        let pnpr_storage = bench_dir.join("pnpr-storage");
        seed_pnpr_auth(&pnpr_storage);
        let public_route_registries = if matches!(self.registry_mode, RegistryMode::Npm) {
            Vec::new()
        } else {
            distinct_public_route_registries([self.registry.as_str(), pnpr_server_registry])
        };
        let pnpr_config =
            write_pnpr_benchmark_config(&bench_dir, &pnpr_storage, &public_route_registries);
        let port = pick_unused_port().expect("pick an unused port for the pnpr server");

        // Wrap the child in its guard *before* anything that can panic
        // (readiness wait, `.pnpr-env` write), so an early failure unwinds
        // through `PnprServer::drop` and kills the process instead of
        // leaking an orphaned server.
        let mut server = PnprServer {
            process: self.spawn_pnpr_server_process(&PnprServerPaths {
                bench_dir: &bench_dir,
                binary: &binary,
                config: &pnpr_config,
                storage: &pnpr_storage,
                port,
            }),
            latency_proxy: None,
        };

        wait_for_pnpr_ready(port);
        // Log in as the seeded benchmark user to mint a bearer token. Real
        // clients authenticate to a pnpr accelerator with `_authToken`, which
        // the server resolves with a fast token lookup rather than a bcrypt on
        // every request (as Basic `_auth` would). Done against the direct port
        // before the latency proxy is interposed, and once per server, so the
        // single login bcrypt stays out of the measured install loop.
        let pnpr_token = mint_pnpr_token(port);

        // With `--pnpr-latency-ms`, the client reaches the server through
        // a latency-injecting proxy instead of directly, so the benchmark
        // measures pnpr as the remote service it is in production. The
        // proxy guard rides along in `PnprServer` so it's torn down with
        // the server.
        let client_url = self.pnpr_client_url(id, port, &mut server);

        self.write_pnpr_client_env(&bench_dir, &client_url, &pnpr_token, pnpr_server_registry);

        server
    }
    /// Write the client config variable and the benchmark's registry rewrite source.
    /// The `PNPM_CONFIG` prefix is required for the server to reach the client config.
    fn write_pnpr_client_env(
        &self,
        bench_dir: &Path,
        client_url: &str,
        pnpr_token: &str,
        pnpr_server_registry: &str,
    ) {
        append_pnpr_auth_to_npmrc(bench_dir, client_url, pnpr_token);
        fs::write(
            bench_dir.join(".pnpr-env"),
            format!(
                "export PNPM_CONFIG_PNPR_SERVER={client_url}\n\
                 export {PNPR_SERVER_REGISTRY_ENV}={pnpr_server_registry}\n\
                 export {PNPR_TARBALL_REWRITE_FROM_ENV}={tarball_rewrite_from}\n",
                tarball_rewrite_from = self.registry,
            ),
        )
        .expect("write .pnpr-env");
    }
    pub(super) fn spawn_pnpr_server_process(&self, paths: &PnprServerPaths<'_>) -> Child {
        eprintln!(
            "Starting pnpr server for {} on 127.0.0.1:{}...",
            paths.bench_dir.display(),
            paths.port,
        );
        let stdout = File::create(paths.bench_dir.join("pnpr-server.stdout.log"))
            .expect("create pnpr server stdout log");
        let stderr = File::create(paths.bench_dir.join("pnpr-server.stderr.log"))
            .expect("create pnpr server stderr log");
        let mut command = Command::new(paths.binary);
        command
            .arg("--config")
            .arg(paths.config)
            .arg("--listen")
            .arg(format!("127.0.0.1:{}", paths.port))
            .arg("--storage")
            .arg(paths.storage)
            // The resolver resolves against the registry the client
            // sends, caching packuments in its own store. A long TTL keeps
            // those cached packuments authoritative across the run, the
            // same value the registry-mock pins for the same reason.
            .arg("--packument-ttl-secs")
            .arg("31536000");
        self.apply_serve_timing(&mut command);
        command
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .expect("spawn pnpr server")
    }
    /// The URL the client reaches the server at: a latency-injecting proxy
    /// when `--pnpr-latency-ms` is set, the server itself otherwise.
    fn pnpr_client_url(&self, id: BenchId, port: u16, server: &mut PnprServer) -> String {
        if self.pnpr_latency_ms == 0 {
            return format!("http://127.0.0.1:{port}");
        }
        let upstream = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        // Latency only: the pnpr resolve protocol exchanges small
        // metadata payloads, so the round trip (not throughput) is the
        // cost that matters for the client↔server link.
        let profile = LinkProfile {
            one_way: Duration::from_millis(self.pnpr_latency_ms) / 2,
            rate_limit: None,
            slow_start: false,
        };
        let proxy = LatencyProxy::spawn(upstream, profile).expect("spawn pnpr latency proxy");
        let proxy_url = format!("http://{}", proxy.addr);
        eprintln!(
            "Injecting {}ms round-trip latency in front of {id}'s server (proxy at {})",
            self.pnpr_latency_ms, proxy.addr,
        );
        server.latency_proxy = Some(proxy);
        proxy_url
    }
    /// Start a proxy in front of an external virtual registry that emulates
    /// a real link (latency + bandwidth cap) for benchmark clients, or
    /// `None` when neither is requested. Spawned Verdaccio registries are
    /// proxied in `main` so their advertised tarball URLs use the proxied
    /// public port.
    pub(super) fn start_client_registry_proxy(&self) -> Option<LatencyProxy> {
        let rate_limit = mbps_to_bytes_per_sec(self.registry_bandwidth_mbps);
        if (self.registry_latency_ms == 0 && rate_limit.is_none())
            || matches!(self.registry_mode, RegistryMode::Npm | RegistryMode::Verdaccio)
        {
            return None;
        }
        let upstream = SocketAddr::from((Ipv4Addr::LOCALHOST, self.registry_port));
        let profile = LinkProfile {
            one_way: Duration::from_millis(self.registry_latency_ms) / 2,
            rate_limit,
            slow_start: self.registry_slow_start,
        };
        let proxy = LatencyProxy::spawn(upstream, profile).expect("spawn registry proxy");
        eprintln!(
            "Fronting the registry with {}ms round-trip latency + {} download cap (proxy at {})",
            self.registry_latency_ms,
            match self.registry_bandwidth_mbps {
                mbps if mbps > 0.0 => format!("{mbps} Mbit/s"),
                _ => "no".to_string(),
            },
            proxy.addr,
        );
        Some(proxy)
    }
    /// Start a latency-only proxy for pnpr server-side registry access.
    /// The client still uses [`Self::start_client_registry_proxy`], which
    /// may have a higher latency and a bandwidth cap for tarball fetches.
    pub(super) fn start_pnpr_server_registry_proxy(&self) -> Option<LatencyProxy> {
        if self.pnpr_server_registry_latency_ms == 0
            || matches!(self.registry_mode, RegistryMode::Npm)
        {
            return None;
        }
        let upstream = SocketAddr::from((Ipv4Addr::LOCALHOST, self.registry_port));
        let profile = LinkProfile {
            one_way: Duration::from_millis(self.pnpr_server_registry_latency_ms) / 2,
            rate_limit: None,
            slow_start: false,
        };
        let proxy =
            LatencyProxy::spawn(upstream, profile).expect("spawn pnpr server registry proxy");
        eprintln!(
            "Fronting the pnpr server registry link with {}ms round-trip latency (proxy at {})",
            self.pnpr_server_registry_latency_ms, proxy.addr,
        );
        Some(proxy)
    }
    /// The registry a given bench id resolves against from the client's
    /// point of view. Direct targets use this for every registry request;
    /// pnpr targets keep it as the materialization registry while their
    /// server receives a separate resolve-registry override in `.pnpr-env`.
    /// The proxy-cache populator may use a separate registry URL for
    /// untimed cache priming.
    ///
    /// When a revision has its own tarball-serving mock (see
    /// [`Self::plan_revision_mocks`]), its `pacquet@<rev>` and `pnpr@<rev>`
    /// targets fetch from that mock instead of the shared one. In the cold-pnpr
    /// scenario both arms therefore exercise the cold mock's serve path — the
    /// direct arm hitting it as a cold pnpr *registry*, the pnpr arm through its
    /// accelerator — and the frozen lockfile keeps that about tarball serving
    /// rather than (noisy) cold resolution.
    pub(super) fn registry_for<'a>(
        &'a self,
        id: BenchId,
        client_registry: &'a str,
        revision_mocks: &'a HashMap<String, RevisionMockRegistry>,
    ) -> &'a str {
        if id.is_proxy_cache_populator() {
            return &self.registry_cache_populator;
        }
        if let Some(mock) = id.revision().and_then(|rev| revision_mocks.get(rev)) {
            return &mock.url;
        }
        client_registry
    }
    /// Assign a per-revision tarball-serving mock to every revision that has
    /// a `pnpr@<rev>` target (so its `pnpr` binary will be built).
    ///
    /// This is what makes a tarball-serve change visible in the `pnpr@HEAD`
    /// vs `pnpr@main` comparison: the shared registry-mock is built from one
    /// revision and serves every arm, so a serve-path delta there cancels out;
    /// giving each revision a mock built from its own `pnpr` exposes the delta.
    ///
    /// Binds the client-facing latency-proxy socket now — reserving the port
    /// for the whole init + build window before `init()` bakes its URL into
    /// `.npmrc` — and hands the live socket to the proxy when `benchmark()`
    /// spawns it. Empty for non-Verdaccio modes, which front no local mock.
    pub(super) fn plan_revision_mocks(&self) -> HashMap<String, RevisionMockRegistry> {
        let mut mocks = HashMap::new();
        if !matches!(self.registry_mode, RegistryMode::Verdaccio) {
            return mocks;
        }
        for target in &self.targets {
            if target.kind != TargetKind::Pnpr {
                continue;
            }
            mocks.entry(target.rev.clone()).or_insert_with(|| {
                let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                    .expect("bind a port for the revision mock proxy");
                let listen_port =
                    listener.local_addr().expect("revision mock proxy local addr").port();
                RevisionMockRegistry { listener, url: format!("http://127.0.0.1:{listen_port}/") }
            });
        }
        mocks
    }
}
