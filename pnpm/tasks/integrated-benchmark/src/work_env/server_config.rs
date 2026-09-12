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
pub(super) struct PnprServerPaths<'a> {
    pub(super) bench_dir: &'a Path,
    pub(super) binary: &'a Path,
    pub(super) config: &'a Path,
    pub(super) storage: &'a Path,
    pub(super) port: u16,
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
pub(super) fn seed_pnpr_auth(pnpr_storage: &Path) {
    fs::create_dir_all(pnpr_storage).expect("create pnpr storage before seeding auth");
    fs::write(pnpr_storage.join("htpasswd"), PNPR_BENCHMARK_HTPASSWD)
        .expect("seed pnpr benchmark htpasswd");
}
pub(super) fn write_pnpr_benchmark_config(
    bench_dir: &Path,
    pnpr_storage: &Path,
    public_route_registries: &[&str],
) -> PathBuf {
    let path = bench_dir.join("pnpr-config.yaml");
    let yaml = pnpr_benchmark_config_yaml(pnpr_storage, public_route_registries);
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
    public_route_registries: &[&str],
) -> String {
    let config = PnprBenchmarkConfig {
        storage: pnpr_storage.display().to_string(),
        secret: "pnpr-integrated-benchmark-secret",
        auth: PnprBenchmarkAuth {
            htpasswd: PnprBenchmarkHtpasswd {
                file: pnpr_storage.join("htpasswd").display().to_string(),
                max_users: -1,
            },
        },
        routes: PnprBenchmarkRoutes {
            public: public_route_registries
                .iter()
                .map(|registry| PnprBenchmarkPublicRoute { registry: (*registry).to_string() })
                .collect(),
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
    let mut file =
        OpenOptions::new().append(true).open(&path).expect("open benchmark .npmrc for pnpr auth");
    writeln!(file, "{}:_authToken={token}", pnpr_auth_config_key(pnpr_server))
        .expect("append pnpr auth to benchmark .npmrc");
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
            payload["token"].as_str().expect("token field in pnpr login response").to_string()
        })
    })
    .join()
    .expect("pnpr token mint thread panicked")
}
pub(super) fn pnpr_auth_config_key(pnpr_server: &str) -> String {
    let Some(without_scheme) =
        pnpr_server.strip_prefix("http://").or_else(|| pnpr_server.strip_prefix("https://"))
    else {
        panic!("pnpr server URL must include a scheme: {pnpr_server}");
    };
    format!("//{}/", without_scheme.trim_end_matches('/'))
}
