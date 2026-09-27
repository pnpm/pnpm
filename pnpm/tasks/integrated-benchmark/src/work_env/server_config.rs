use super::{PNPR_BENCHMARK_HTPASSWD, PNPR_BENCHMARK_PASSWORD, PNPR_BENCHMARK_USERNAME};
use crate::latency_proxy::LatencyProxy;
use serde::Serialize;
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Child,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

/// Where a revision's tarball-serving mock lives: the latency proxy clients
/// reach it through and the URL form of its port.
#[derive(Debug)]
pub(super) struct RevisionMockRegistry {
    /// The client↔registry latency proxy's listening socket, **bound at
    /// planning time** so the port is reserved for the whole init + build
    /// window and can't be stolen before `benchmark()` hands it to
    /// [`LatencyProxy::spawn_with_listener`].
    pub(super) listener: TcpListener,
    /// `http://127.0.0.1:<port>/` — the registry URL baked into each target's
    /// `.npmrc` at `init()`, before the mock itself exists.
    pub(super) url: String,
}
/// A pnpr resolver server spawned for one `pnpr@<rev>`
/// target. Killed on drop so it never outlives the benchmark run.
/// Where one benchmark's pnpr server binary, config and storage live.
pub(super) struct PnprServerPaths {
    pub(super) bench_dir: PathBuf,
    pub(super) binary: PathBuf,
    pub(super) config: PathBuf,
    pub(super) storage: PathBuf,
    pub(super) port: u16,
}
/// A `pnpr@<rev>` target's resolver server, with the paths that launch it
/// again after a restart on empty storage.
pub(super) struct PnprResolverServer {
    pub(super) server: PnprServer,
    pub(super) paths: PnprServerPaths,
}
pub(super) struct PnprServer {
    pub(super) process: Child,
    /// The latency proxy fronting this server, when `--pnpr-latency-ms`
    /// is set. Dropped (stopping the proxy) alongside the server.
    pub(super) latency_proxy: Option<LatencyProxy>,
}
impl Drop for PnprServer {
    fn drop(&mut self) {
        // Drop the proxy first (it stops accepting new connections and
        // joins its accept loop), then kill the server. By the time a
        // server is torn down the benchmark has finished, so there are no
        // in-flight connections to drain; this is just tidy teardown
        // order, not a guarantee that existing connections are flushed.
        self.latency_proxy = None;
        let pid = self.process.id();
        let _ = self.process.kill();
        let _ = self.process.wait();
        eprintln!("info: Terminated pnpr server pid {pid}");
    }
}
/// The loopback socket through which hyperfine's `--prepare` asks the
/// benchmark to restart its pnpr servers. The restart runs in this process,
/// so every server stays a [`PnprServer`] guard that dies with the run.
pub(super) struct PnprRestartListener {
    listener: TcpListener,
    stopped: AtomicBool,
}
impl PnprRestartListener {
    const ACK: &'static str = "restarted";

    pub(super) fn bind() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind the pnpr restart listener");
        PnprRestartListener { listener, stopped: AtomicBool::new(false) }
    }

    /// Serve restart requests on a scoped thread while `run` executes with a
    /// `--prepare` step that sends one. The step blocks until `restart` has
    /// finished and fails when no acknowledgement arrives, so an iteration
    /// never runs against a server that was not restarted.
    pub(super) fn serve_during(&self, restart: impl FnMut() + Send, run: impl FnOnce(&str)) {
        thread::scope(|scope| {
            scope.spawn(|| self.serve(restart));
            let _stop = StopOnDrop(self);
            run(&self.prepare_command());
        });
    }

    fn prepare_command(&self) -> String {
        let port = self.listener
            .local_addr()
            .expect("pnpr restart listener address")
            .port();
        format!(
            r#"bash -c 'exec 3<>/dev/tcp/127.0.0.1/{port} && read -r reply <&3 && [ "$reply" = {ack} ]'"#,
            ack = Self::ACK,
        )
    }

    fn serve(&self, mut restart: impl FnMut()) {
        for stream in self.listener.incoming() {
            if self.stopped.load(Ordering::Acquire) {
                return;
            }
            let mut stream = stream.expect("accept a pnpr restart request");
            restart();
            writeln!(stream, "{}", Self::ACK).expect("acknowledge the pnpr restart");
        }
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        let addr = self.listener.local_addr().expect("pnpr restart listener address");
        // Wakes the blocking accept in `serve`. Fails harmlessly once it has returned.
        let _ = TcpStream::connect(addr);
    }
}
/// Stops the listener even while a panic unwinds out of `run`, so the scope
/// in [`PnprRestartListener::serve_during`] can join its serving thread.
struct StopOnDrop<'a>(&'a PnprRestartListener);
impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.stop();
    }
}
/// Poll the pnpr server's TCP port until it accepts a connection. Stays
/// dependency-free (no async HTTP client) because the orchestrator's
/// benchmark path is synchronous, unlike the registry-mock's spawn.
pub(super) fn wait_for_pnpr_ready(port: u16) {
    const MAX_RETRIES: usize = 40;
    const RETRY_DELAY: Duration = Duration::from_millis(250);
    for _ in 0..MAX_RETRIES {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        thread::sleep(RETRY_DELAY);
    }
    panic!("pnpr server on 127.0.0.1:{port} did not become ready");
}
/// Where a pnpr target's `htpasswd` lives. pnpr keeps `tokens.db` next to it,
/// so keeping both outside `pnpr-storage` lets a restart on empty storage
/// accept the token minted at startup.
pub(super) fn pnpr_htpasswd_path(bench_dir: &Path) -> PathBuf {
    bench_dir.join("pnpr-auth").join("htpasswd")
}
pub(super) fn seed_pnpr_auth(htpasswd: &Path) {
    let auth_dir = htpasswd.parent().expect("htpasswd path has a parent");
    fs::create_dir_all(auth_dir).expect("create pnpr auth dir before seeding auth");
    fs::write(htpasswd, PNPR_BENCHMARK_HTPASSWD).expect("seed pnpr benchmark htpasswd");
}
pub(super) fn write_pnpr_benchmark_config(
    bench_dir: &Path,
    pnpr_storage: &Path,
    public_route_registries: &[&str],
) -> PathBuf {
    let path = bench_dir.join("pnpr-config.yaml");
    let yaml = pnpr_benchmark_config_yaml(
        pnpr_storage,
        &pnpr_htpasswd_path(bench_dir),
        public_route_registries,
    );
    fs::write(&path, yaml).expect("write pnpr benchmark config");
    path
}
/// A config for the cold mock: isolated `storage` and a single public upstream
/// at the warm origin, so every request is a cache miss proxied through to it.
///
/// The routing is expressed in *two* shapes so one file drives every
/// benchmarked `pnpr` binary back to the mount model: the `registries:` +
/// `defaultRegistry:` model that current `pnpr` reads, and the `mounts:` +
/// `defaultTarget:` shape it replaced. Each server ignores the block it
/// doesn't recognize, so both proxy every request to the same `origin`. The
/// pre-mount `uplinks:` + `packages: proxy:` shape can no longer ride along:
/// current `pnpr` deliberately *rejects* a top-level `packages:` block at
/// startup (per-package rules live on each registry now, and silently
/// dropping a formerly-enforced ACL would be a security regression), so a
/// revision older than the mount model cannot share a config file with
/// current ones.
pub(super) fn cold_mock_config_yaml(storage: &Path, origin: &str) -> String {
    let upstream =
        || ColdMockUpstreamEntry { kind: "upstream", url: origin.to_string(), public: true };
    let config = ColdMockConfig {
        storage: storage.display().to_string(),
        mounts: ColdMockUpstreamTable { npmjs: upstream() },
        default_target: "npmjs",
        registries: ColdMockUpstreamTable { npmjs: upstream() },
        default_registry: "npmjs",
        log: ColdMockLog { kind: "stdout", format: "pretty", level: "error" },
    };
    serde_saphyr::to_string(&config).expect("serialize cold mock config")
}
/// The cold mock config, serialized rather than string-formatted so the
/// `storage` path and `origin` URL are escaped and the structure can't drift.
#[derive(Serialize)]
pub(super) struct ColdMockConfig {
    storage: String,
    mounts: ColdMockUpstreamTable,
    #[serde(rename = "defaultTarget")]
    default_target: &'static str,
    registries: ColdMockUpstreamTable,
    #[serde(rename = "defaultRegistry")]
    default_registry: &'static str,
    log: ColdMockLog,
}
/// The `mounts:` / `registries:` table — the same single-upstream entry under
/// either key, since the registries model kept the tagged-entry shape.
#[derive(Serialize)]
pub(super) struct ColdMockUpstreamTable {
    npmjs: ColdMockUpstreamEntry,
}
/// One `mounts:`/`registries:` entry: the `type:` tag selects the kind
/// (`upstream` here), and the kind's fields sit alongside it.
#[derive(Serialize)]
pub(super) struct ColdMockUpstreamEntry {
    #[serde(rename = "type")]
    kind: &'static str,
    url: String,
    public: bool,
}
#[derive(Serialize)]
pub(super) struct ColdMockLog {
    #[serde(rename = "type")]
    kind: &'static str,
    format: &'static str,
    level: &'static str,
}
pub(super) fn pnpr_benchmark_config_yaml(
    pnpr_storage: &Path,
    htpasswd: &Path,
    public_route_registries: &[&str],
) -> String {
    let config = PnprBenchmarkConfig {
        storage: pnpr_storage.display().to_string(),
        secret: "pnpr-integrated-benchmark-secret",
        auth: PnprBenchmarkAuth {
            htpasswd: PnprBenchmarkHtpasswd { file: htpasswd.display().to_string(), max_users: -1 },
        },
        routes: PnprBenchmarkRoutes {
            public: public_route_registries
                .iter()
                .map(|registry| PnprBenchmarkPublicRoute { registry: (*registry).to_string() })
                .collect(),
            allowed_private_networks: &["127.0.0.0/8", "::1"],
        },
        log: PnprBenchmarkLog { r#type: "stdout", format: "pretty", level: "error" },
    };
    serde_saphyr::to_string(&config).expect("serialize pnpr benchmark config")
}
pub(super) fn distinct_public_route_registries<const REGISTRY_COUNT: usize>(
    registries: [&str; REGISTRY_COUNT],
) -> Vec<&str> {
    let mut distinct = Vec::with_capacity(registries.len());
    for registry in registries {
        if !distinct.contains(&registry) {
            distinct.push(registry);
        }
    }
    distinct
}
#[derive(Serialize)]
pub(super) struct PnprBenchmarkConfig {
    storage: String,
    secret: &'static str,
    auth: PnprBenchmarkAuth,
    routes: PnprBenchmarkRoutes,
    log: PnprBenchmarkLog,
}
#[derive(Serialize)]
pub(super) struct PnprBenchmarkAuth {
    htpasswd: PnprBenchmarkHtpasswd,
}
#[derive(Serialize)]
pub(super) struct PnprBenchmarkHtpasswd {
    file: String,
    max_users: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PnprBenchmarkRoutes {
    public: Vec<PnprBenchmarkPublicRoute>,
    /// The mocks and proxies the resolver fetches from listen on loopback.
    /// A `pnpr` that predates the setting ignores it.
    allowed_private_networks: &'static [&'static str],
}
#[derive(Serialize)]
pub(super) struct PnprBenchmarkPublicRoute {
    registry: String,
}
#[derive(Serialize)]
pub(super) struct PnprBenchmarkLog {
    r#type: &'static str,
    format: &'static str,
    level: &'static str,
}
pub(super) fn append_pnpr_auth_to_npmrc(dir: &Path, pnpr_server: &str, token: &str) {
    let path = dir.join(".npmrc");
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open benchmark .npmrc for pnpr auth");
    writeln!(file, "{}:_authToken={token}", pnpr_auth_config_key(pnpr_server)).expect(
        "append pnpr auth to benchmark .npmrc",
    );
}
/// Log in as the seeded benchmark user and return a bearer token. The login
/// runs on a dedicated thread with its own runtime so it doesn't reach into the
/// harness's ambient `#[tokio::main]` runtime (which would make a blocking
/// client panic).
pub(super) fn mint_pnpr_token(port: u16) -> String {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("runtime for pnpr token mint");
        runtime.block_on(async move {
            let url = format!(
                "http://127.0.0.1:{port}/-/user/org.couchdb.user:{PNPR_BENCHMARK_USERNAME}",
            );
            let body = serde_json::json!({
                "_id": format!("org.couchdb.user:{PNPR_BENCHMARK_USERNAME}"),
                "name": PNPR_BENCHMARK_USERNAME,
                "password": PNPR_BENCHMARK_PASSWORD,
                "type": "user",
                "roles": [],
            });
            let response = reqwest::Client::new()
                .put(&url)
                .json(&body)
                .send()
                .await
                .expect("log in to pnpr to mint a benchmark token");
            assert!(
                response.status().is_success(),
                "pnpr login returned {} when minting a benchmark token",
                response.status(),
            );
            let payload: Value = response.json().await.expect("parse pnpr login response");
            payload["token"]
                .as_str()
                .expect("token field in pnpr login response")
                .to_string()
        })
    })
    .join()
    .expect("pnpr token mint thread panicked")
}
pub(super) fn pnpr_auth_config_key(pnpr_server: &str) -> String {
    let Some(without_scheme) = pnpr_server
        .strip_prefix("http://")
        .or_else(|| pnpr_server.strip_prefix("https://"))
    else {
        panic!("pnpr server URL must include a scheme: {pnpr_server}");
    };
    format!("//{}/", without_scheme.trim_end_matches('/'))
}
