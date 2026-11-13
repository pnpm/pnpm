use super::{
    PNPR_BENCHMARK_PASSWORD, PNPR_BENCHMARK_USERNAME,
    registries_home::{LOOPBACK_NETWORKS, RegistriesHome},
};
use crate::latency_proxy::LatencyProxy;
use serde::Serialize;
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::Duration,
};

#[cfg(test)]
mod tests;

/// Where a revision's tarball-serving mock lives: the latency proxy clients
/// reach it through and the URL form of its port.
#[derive(Debug)]
pub(super) struct RevisionMockRegistry {
    /// The client↔registry latency proxy's listening socket, **bound at
    /// planning time** so the port is reserved for the whole init + build
    /// window and can't be stolen before `benchmark()` hands it to
    /// [`LatencyProxy::spawn_with_listener`].
    pub(super) listener: TcpListener,
    /// `http://127.0.0.1:<port>`, the mock's `--public-url`.
    pub(super) origin: String,
    /// The npm registry URL under `origin`, baked into each target's `.npmrc`
    /// at `init()`, before the mock itself exists.
    pub(super) url: String,
}
/// Where one benchmark's pnpr server binary, config and storage live.
pub(super) struct PnprServerPaths<'a> {
    pub(super) bench_dir: &'a Path,
    pub(super) binary: &'a Path,
    pub(super) config: &'a Path,
    /// The registries document to import before the server starts, for a
    /// revision whose config file holds no registries.
    pub(super) registries: Option<&'a Path>,
    pub(super) storage: &'a Path,
    pub(super) port: u16,
}
/// A pnpr resolver server spawned for one `pnpr@<rev>`
/// target. Killed on drop so it never outlives the benchmark run.
pub(super) struct PnprServer {
    pub(super) process: Child,
    /// The latency proxy fronting this server, when `--pnpr-latency-ms`
    /// is set. Dropped (stopping the proxy) alongside the server.
    pub(super) latency_proxy: Option<LatencyProxy>,
    /// Warm origin server backing a cold revision mock, spawned from the same
    /// revision's `pnpr` binary so each revision speaks its own packument and
    /// tarball URL format to its upstream.
    pub(super) origin: Option<Box<PnprServer>>,
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
        self.origin = None;
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
/// The files one benchmark's pnpr server reads.
pub(super) struct PnprBenchmarkFiles {
    pub(super) config: PathBuf,
    /// The `routes.public` document to import, for a revision that reads
    /// its registries from the database.
    pub(super) registries: Option<PathBuf>,
}
pub(super) fn write_pnpr_benchmark_config(
    bench_dir: &Path,
    pnpr_storage: &Path,
    public_route_registries: &[&str],
) -> PnprBenchmarkFiles {
    let config = bench_dir.join("pnpr-config.yaml");
    let source_dir = bench_dir.join("pacquet");
    let source = fs::read_to_string(source_dir.join("pnpr/.fixtures/config.yaml"))
        .expect("read the revision account configuration");
    // Beside the storage: pnpr refuses a hosted store holding a directory
    // it did not write.
    let auth = super::revision_auth::seed_auth(&source, bench_dir);
    let home = RegistriesHome::of_source(&source_dir);
    let yaml = pnpr_benchmark_config_yaml(pnpr_storage, public_route_registries, auth, home);
    fs::write(&config, yaml).expect("write pnpr benchmark config");
    let registries = home
        .in_database()
        .then(|| {
            let path = bench_dir.join("pnpr-registries.yaml");
            fs::write(&path, pnpr_benchmark_registries_yaml(public_route_registries))
                .expect("write pnpr benchmark registries");
            path
        });
    PnprBenchmarkFiles { config, registries }
}
/// The `--public-url` of a benchmark pnpr server whose registries live in
/// the database. Its resolver reaches its own host only at this
/// origin, and the registries it resolves from listen on `localhost` and
/// `127.0.0.1`, so the URL names a host neither spells. Clients reach the
/// server at `127.0.0.1`; nothing they fetch is built from this URL, since
/// the server hosts and proxies no registry.
pub(super) fn pnpr_benchmark_public_url(port: u16) -> String {
    format!("http://pnpr-benchmark.localhost:{port}")
}
/// A revision mock to start.
pub(super) struct RevisionMockLaunch<'a> {
    pub(super) revision: &'a str,
    pub(super) binary: &'a Path,
    pub(super) bench_dir: &'a Path,
    pub(super) port: u16,
    pub(super) public_url: &'a str,
    pub(super) registries: RegistriesHome,
}
/// A config for the cold mock: isolated storage and cache, with a public
/// upstream at the warm origin. Only the cache is cleared between iterations.
///
/// The routing is expressed with `registries:` + `defaultRegistry:` in the
/// config, or in the document [`cold_mock_registries_yaml`] writes.
pub(super) fn cold_mock_config_yaml(
    storage: &Path,
    origin: &str,
    registries: RegistriesHome,
) -> String {
    let graph = match registries {
        RegistriesHome::ConfigFile => Some(cold_mock_graph(origin)),
        RegistriesHome::Database => None,
    };
    let config = ColdMockConfig {
        storage: storage.display().to_string(),
        cache: storage
            .join("cache")
            .display()
            .to_string(),
        registries: graph.as_ref().map(|graph| &graph.registries),
        default_registry: graph.as_ref().map(|graph| graph.default_registry),
        routes: registries
            .in_database()
            .then_some(LoopbackRoutes { allowed_private_networks: LOOPBACK_NETWORKS }),
        log: ColdMockLog { format: "pretty", level: "error" },
    };
    serde_saphyr::to_string(&config).expect("serialize cold mock config")
}
/// The cold mock's registries, for a revision that reads them from the
/// database.
pub(super) fn cold_mock_registries_yaml(origin: &str) -> String {
    serde_saphyr::to_string(&cold_mock_graph(origin)).expect("serialize cold mock registries")
}
fn cold_mock_graph(origin: &str) -> ColdMockGraph {
    let npmjs = ColdMockUpstreamEntry { kind: "upstream", url: origin.to_string(), public: true };
    ColdMockGraph { registries: ColdMockUpstreamTable { npmjs }, default_registry: "npmjs" }
}
/// Use an isolated cache, cleared between iterations, with the warm mock as
/// origin. For [`RegistriesHome::Database`], `mock.public_url` must be on
/// another host than `origin_url`.
pub(super) fn cold_revision_mock_command(
    mock: &RevisionMockLaunch<'_>,
    origin_url: &str,
) -> Command {
    let cold_storage = mock.bench_dir.join("cold-mock-storage");
    let config_path = mock.bench_dir.join("cold-mock-config.yaml");
    fs::write(&config_path, cold_mock_config_yaml(&cold_storage, origin_url, mock.registries))
        .expect("write cold mock config");
    eprintln!(
        "Serving {}'s tarballs from a COLD mock built from pnpr@{} on 127.0.0.1:{} (origin {origin_url})...",
        mock.revision, mock.revision, mock.port,
    );
    let mut command = Command::new(mock.binary);
    command
        .arg("--config")
        .arg(&config_path)
        .arg("--storage")
        .arg(&cold_storage)
        .arg("--cache")
        .arg(cold_storage.join("cache"))
        .arg("--listen")
        .arg(format!("127.0.0.1:{}", mock.port))
        .arg("--public-url")
        .arg(mock.public_url)
        .arg("--packument-ttl")
        .arg("31536000");
    if mock.registries.in_database() {
        let document = mock.bench_dir.join("cold-mock-registries.yaml");
        fs::write(&document, cold_mock_registries_yaml(origin_url))
            .expect("write cold mock registries");
        pnpm_registry_mock::import_registries(&command, &document);
    }
    command
}
/// The cold mock config, serialized rather than string-formatted so the
/// `storage` path and `origin` URL are escaped and the structure can't drift.
#[derive(Serialize)]
pub(super) struct ColdMockConfig<'a> {
    storage: String,
    cache: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    registries: Option<&'a ColdMockUpstreamTable>,
    #[serde(rename = "defaultRegistry", skip_serializing_if = "Option::is_none")]
    default_registry: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    routes: Option<LoopbackRoutes>,
    log: ColdMockLog,
}
/// The cold mock's registries, in the shape of a registries document.
#[derive(Serialize)]
pub(super) struct ColdMockGraph {
    registries: ColdMockUpstreamTable,
    #[serde(rename = "defaultRegistry")]
    default_registry: &'static str,
}
/// The `registries:` table — a single-upstream entry.
#[derive(Serialize)]
pub(super) struct ColdMockUpstreamTable {
    npmjs: ColdMockUpstreamEntry,
}
/// One `registries:` entry: the `type:` tag selects the kind
/// (`upstream` here), and the kind's fields sit alongside it.
#[derive(Serialize)]
pub(super) struct ColdMockUpstreamEntry {
    #[serde(rename = "type")]
    kind: &'static str,
    url: String,
    public: bool,
}
/// `routes:` with only `allowedPrivateNetworks`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LoopbackRoutes {
    allowed_private_networks: &'static [&'static str],
}
#[derive(Serialize)]
pub(super) struct ColdMockLog {
    format: &'static str,
    level: &'static str,
}
pub(super) fn pnpr_benchmark_config_yaml(
    pnpr_storage: &Path,
    public_route_registries: &[&str],
    auth: Value,
    registries: RegistriesHome,
) -> String {
    let config = PnprBenchmarkConfig {
        storage: pnpr_storage.display().to_string(),
        secret: "pnpr-integrated-benchmark-secret",
        auth,
        routes: PnprBenchmarkRoutes {
            public: match registries {
                RegistriesHome::ConfigFile => Some(public_routes(public_route_registries)),
                RegistriesHome::Database => None,
            },
            allowed_private_networks: LOOPBACK_NETWORKS,
        },
        log: PnprBenchmarkLog { format: "pretty", level: "error" },
    };
    serde_saphyr::to_string(&config).expect("serialize pnpr benchmark config")
}
/// The registries document holding only the `routes.public` entries.
pub(super) fn pnpr_benchmark_registries_yaml(public_route_registries: &[&str]) -> String {
    let document = PnprBenchmarkGraph {
        routes: PnprBenchmarkGraphRoutes { public: public_routes(public_route_registries) },
    };
    serde_saphyr::to_string(&document).expect("serialize pnpr benchmark registries")
}
fn public_routes(registries: &[&str]) -> Vec<PnprBenchmarkPublicRoute> {
    registries
        .iter()
        .map(|registry| PnprBenchmarkPublicRoute { registry: (*registry).to_string() })
        .collect()
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
    auth: Value,
    routes: PnprBenchmarkRoutes,
    log: PnprBenchmarkLog,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PnprBenchmarkRoutes {
    #[serde(skip_serializing_if = "Option::is_none")]
    public: Option<Vec<PnprBenchmarkPublicRoute>>,
    /// The mocks and proxies the resolver fetches from listen on loopback.
    /// A `pnpr` that predates the setting ignores it.
    allowed_private_networks: &'static [&'static str],
}
#[derive(Serialize)]
pub(super) struct PnprBenchmarkGraph {
    routes: PnprBenchmarkGraphRoutes,
}
#[derive(Serialize)]
pub(super) struct PnprBenchmarkGraphRoutes {
    public: Vec<PnprBenchmarkPublicRoute>,
}
#[derive(Serialize)]
pub(super) struct PnprBenchmarkPublicRoute {
    registry: String,
}
#[derive(Serialize)]
pub(super) struct PnprBenchmarkLog {
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
/// Run `future` to completion on a dedicated thread with its own runtime, so
/// the synchronous benchmark path doesn't reach into the harness's ambient
/// `#[tokio::main]` runtime (which would make a blocking client panic).
fn block_on_own_runtime<Output: Send + 'static>(
    future: impl Future<Output = Output> + Send + 'static,
) -> Output {
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .expect("create a runtime for a benchmark request")
            .block_on(future)
    })
    .join()
    .expect("benchmark request thread panicked")
}
/// Whether the pnpr server on `port` serves npm under a path of its own, per
/// its registry directory. A server whose directory is missing or does not
/// say so serves npm at its root.
pub(super) fn serves_npm_under_base_path(port: u16) -> bool {
    block_on_own_runtime(async move {
        let url = format!("http://127.0.0.1:{port}/-/pnpr/v0/registries");
        let response = reqwest::get(&url).await
            .unwrap_or_else(|error| panic!("query the registry directory at {url}: {error}"));
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return false;
        }
        assert!(
            response.status().is_success(),
            "the registry directory at {url} returned {}",
            response.status(),
        );
        let directory: Value = response.json().await.expect("parse the registry directory");
        directory["ecosystems"]["npm"]["prefixed"].as_bool() == Some(true)
    })
}
/// Log in as the seeded benchmark user and return a bearer token.
pub(super) fn mint_pnpr_token(port: u16) -> String {
    block_on_own_runtime(async move {
        let url =
            format!("http://127.0.0.1:{port}/-/user/org.couchdb.user:{PNPR_BENCHMARK_USERNAME}");
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
