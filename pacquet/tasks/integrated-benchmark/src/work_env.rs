use crate::{
    cli_args::{
        BenchmarkScenario, Cleanup, HyperfineOptions, RegistryMode, TargetKind, TargetSpec,
    },
    fixtures::{LOCKFILE, PACKAGE_JSON},
    latency_proxy::{LatencyProxy, LinkProfile, mbps_to_bytes_per_sec},
    verify::executor,
    workspace_manifest::MinimalWorkspaceManifest,
};
use itertools::Itertools;
use os_display::Quotable;
use pacquet_fs::file_mode::make_file_executable;
use pacquet_registry_mock::pick_unused_port;
use pipe_trait::Pipe;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    borrow::Cow,
    collections::HashMap,
    fmt::{self, Write as _},
    fs::{self, File},
    io::Write,
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

const BENCHMARK_OUTPUT_LOG: &str = "BENCHMARK_OUTPUT.ndjson";
const BENCHMARK_DIAGNOSTICS_JSON: &str = "BENCHMARK_DIAGNOSTICS.json";
const BENCHMARK_DIAGNOSTICS_MD: &str = "BENCHMARK_DIAGNOSTICS.md";
const PNPR_DIRECT_RATIO_MAX: f64 = 1.05;
const PNPR_SERVER_REGISTRY_ENV: &str = "PACQUET_BENCHMARK_PNPR_SERVER_REGISTRY";
const PNPR_TARBALL_REWRITE_FROM_ENV: &str = "PACQUET_BENCHMARK_PNPR_TARBALL_REWRITE_FROM";

#[derive(Debug)]
pub struct WorkEnv {
    pub root: PathBuf,
    pub with_pnpm: bool,
    pub targets: Vec<TargetSpec>,
    /// Registry URL used by benchmarked clients.
    pub registry: String,
    /// Registry URL used only by the pre-benchmark cache populator.
    pub registry_cache_populator: String,
    pub registry_mode: RegistryMode,
    pub repository: PathBuf,
    pub pnpm_repository: Option<PathBuf>,
    pub scenario: Option<BenchmarkScenario>,
    pub hyperfine_options: HyperfineOptions,
    pub fixture_dir: Option<PathBuf>,
    /// Round-trip latency (ms) to inject between the client and each
    /// `pnpr@<rev>` target's server. `0` leaves the server on loopback.
    pub pnpr_latency_ms: u64,
    /// Round-trip latency (ms) on the link to the registry, applied to
    /// every client (direct installs and the pnpr server + client alike).
    /// `0` leaves the registry on loopback. Ignored in `--registry=npm`
    /// mode (already remote).
    pub registry_latency_ms: u64,
    /// Round-trip latency (ms) between a pnpr server and the registry it
    /// resolves against. Separate from `registry_latency_ms` so the
    /// benchmark can model a co-located server with fast metadata access
    /// while clients still fetch tarballs over a remote link. Ignored in
    /// `--registry=npm` mode.
    pub pnpr_server_registry_latency_ms: u64,
    /// Download-bandwidth cap (megabits/sec) on the link to the registry,
    /// applied to every client, so tarball fetches cost real time instead
    /// of being free on loopback. `0` leaves the registry at loopback
    /// speed. Ignored in `--registry=npm` mode (already remote).
    pub registry_bandwidth_mbps: f64,
    pub registry_slow_start: bool,
    /// Port the local registry listens on, used as the proxy's upstream
    /// when latency or a bandwidth cap is requested.
    pub registry_port: u16,
    /// Skip the clone + `cargo build` for a target whose output binary is
    /// already present — i.e. restored from a per-commit CI cache. Off by
    /// default so a local run always rebuilds.
    pub reuse_prebuilt_binaries: bool,
}

impl WorkEnv {
    const INIT_PROXY_CACHE: BenchId<'static> = BenchId::Static(INIT_PROXY_CACHE_ID);
    const SYSTEM_PNPM: BenchId<'static> = BenchId::Static("pnpm");

    fn root(&self) -> &'_ Path {
        &self.root
    }

    fn target_ids(&self) -> impl Iterator<Item = BenchId<'_>> + '_ {
        self.targets.iter().map(BenchId::from)
    }

    /// Every bench dir the run will touch — every target plus, when
    /// requested, the system-pnpm sibling.
    fn benchmarked_ids(&self) -> impl Iterator<Item = BenchId<'_>> + '_ {
        self.target_ids().chain(self.with_pnpm.then_some(WorkEnv::SYSTEM_PNPM))
    }

    fn repository(&self) -> &'_ Path {
        &self.repository
    }

    /// Repository to fetch pnpm revisions from. Falls back to the
    /// pacquet repo when the caller didn't override it — useful when
    /// the same monorepo checkout contains both code bases.
    fn pnpm_repository(&self) -> &'_ Path {
        self.pnpm_repository.as_deref().unwrap_or_else(|| self.repository())
    }

    fn bench_dir(&self, id: BenchId) -> PathBuf {
        self.root().join(id.to_string())
    }

    fn script_path(&self, id: BenchId) -> PathBuf {
        self.bench_dir(id).join("install.bash")
    }

    fn bash_command(&self, id: BenchId) -> String {
        // Hyperfine runs each command through a shell, so the script
        // path needs to survive shell-tokenization. `maybe_quote()`
        // wraps the path in single quotes (and escapes any embedded
        // quotes) when it contains a metacharacter — leaves it bare
        // when the path is alphanumeric/slash/dash only, which is the
        // common case.
        format!("bash {}", self.script_path(id).maybe_quote())
    }

    /// Source-tree location for a pacquet revision: `<bench_dir>/pacquet`.
    fn pacquet_source_dir(&self, revision: &str) -> PathBuf {
        self.bench_dir(BenchId::PacquetRevision(revision)).join("pacquet")
    }

    /// Source-tree location for a pnpr revision: `<bench_dir>/pacquet`.
    /// A pnpr target builds from the same monorepo clone as a pacquet
    /// target (the `pacquet` and `pnpr` crates share one workspace), so
    /// the layout matches [`Self::pacquet_source_dir`].
    fn pnpr_source_dir(&self, revision: &str) -> PathBuf {
        self.bench_dir(BenchId::PnprRevision(revision)).join("pacquet")
    }

    /// Source-tree location for a pnpm revision: `<bench_dir>/pnpm-source`.
    fn pnpm_source_dir(&self, revision: &str) -> PathBuf {
        self.bench_dir(BenchId::PnpmRevision(revision)).join("pnpm-source")
    }

    fn resolve_revision(repository: &Path, revision: &str) -> String {
        let output = Command::new("git")
            .current_dir(repository)
            .arg("rev-parse")
            .arg(revision)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .output()
            .expect("git rev-parse");
        assert!(output.status.success());
        output
            .stdout
            .pipe(String::from_utf8)
            .expect("output of rev-parse is valid UTF-8")
            .trim()
            .to_string()
    }

    /// Shell command (with arguments) that runs the install for `id`,
    /// embedded into the per-target `install.bash` script. The result
    /// is intentionally `String` because pnpm targets prefix `node` in
    /// front of a runtime-resolved bundle path.
    ///
    /// The script `cd`s into the bench dir before exec-ing this, so
    /// every path here is relative to the bench dir — that lets the
    /// pnpm-target branch defer `pnpm.mjs` vs `pnpm.cjs` resolution
    /// to script runtime (after `build()` has produced the bundle),
    /// and also avoids embedding absolute paths that might contain
    /// shell metacharacters from a user-supplied `--work-env`.
    fn install_command(id: BenchId) -> String {
        match id {
            // A pnpr target runs the same pacquet client binary; the
            // pnpr server it talks to is started separately at benchmark
            // time and reached via the `PNPM_CONFIG_PNPR_SERVER` env var
            // that `install.bash` sources from `.pnpr-env`.
            BenchId::PacquetRevision(_) | BenchId::PnprRevision(_) => {
                "./pacquet/target/release/pacquet".to_string()
            }
            BenchId::PnpmRevision(_) => {
                // Prefer `pnpm.mjs`, fall back to `pnpm.cjs`. Resolved
                // at script runtime so the existence check sees the
                // bundle produced by `pnpm run compile-only`, not the empty
                // tree visible during `init()`. Mirrors
                // `resolve_pnpm_bin` in `benchmarks/bench.sh`.
                let mjs = "./pnpm-source/pnpm/dist/pnpm.mjs";
                let cjs = "./pnpm-source/pnpm/dist/pnpm.cjs";
                format!(r#"node "$([ -f {mjs} ] && echo {mjs} || echo {cjs})""#)
            }
            BenchId::Static(_) => "pnpm".to_string(),
        }
    }

    fn init(&self, direct_registry: &str) {
        let scenario = self.scenario.expect("scenario set when init() is reached");
        eprintln!("Initializing...");
        // The proxy-cache populator only runs against a local
        // verdaccio/virtual registry to warm its on-disk cache. With
        // `--registry=npm`, no proxy exists, so skip writing the
        // INIT_PROXY_CACHE bench dir entirely — its files (npmrc,
        // workspace, install script, saved-pristine copies) would be
        // unreferenced overhead.
        let populate_proxy_cache =
            matches!(self.registry_mode, RegistryMode::Verdaccio | RegistryMode::Virtual);
        let id_list = self
            .target_ids()
            .chain(populate_proxy_cache.then_some(WorkEnv::INIT_PROXY_CACHE))
            .chain(self.with_pnpm.then_some(WorkEnv::SYSTEM_PNPM));
        for id in id_list {
            eprintln!("ID: {id}");
            let dir = self.bench_dir(id);
            let registry = self.registry_for(id, direct_registry);
            fs::create_dir_all(&dir).expect("create directory for the revision");
            create_package_json(&dir, self.fixture_dir.as_deref());
            create_pnpm_workspace(&dir, self.fixture_dir.as_deref(), registry, scenario);
            create_install_script(&dir, scenario, &WorkEnv::install_command(id), id);
            create_npmrc(&dir, registry, scenario);
            may_create_lockfile(&dir, scenario, self.fixture_dir.as_deref());
            save_pristine_copies(&dir);
        }

        if populate_proxy_cache {
            eprintln!("Populating proxy registry cache...");
            Command::new("bash")
                .arg(self.script_path(WorkEnv::INIT_PROXY_CACHE))
                .pipe_mut(executor("install.bash"));
        }
    }

    /// Output binary a `pacquet@<rev>` target runs.
    fn pacquet_binary(&self, revision: &str) -> PathBuf {
        self.pacquet_source_dir(revision).join("target").join("release").join("pacquet")
    }

    /// The `pacquet` client binary a `pnpr@<rev>` build produces (the pnpr
    /// target builds `--bin=pacquet --bin=pnpr`).
    fn pnpr_pacquet_binary(&self, revision: &str) -> PathBuf {
        self.pnpr_source_dir(revision).join("target").join("release").join("pacquet")
    }

    /// The `pnpr` server binary a `pnpr@<rev>` build produces.
    fn pnpr_server_binary(&self, revision: &str) -> PathBuf {
        self.pnpr_source_dir(revision).join("target").join("release").join("pnpr")
    }

    pub fn build(&self) {
        eprintln!("Building...");
        // Build `pnpr@<rev>` targets first: a pnpr build also produces the
        // `pacquet` client binary, so a same-revision `pacquet@<rev>` can
        // reuse it (see [`Self::build_pacquet`]) instead of compiling the
        // identical commit a second time.
        let pnpr_first = self
            .targets
            .iter()
            .filter(|target| target.kind == TargetKind::Pnpr)
            .chain(self.targets.iter().filter(|target| target.kind != TargetKind::Pnpr));
        for target in pnpr_first {
            match target.kind {
                TargetKind::Pacquet => self.build_pacquet(&target.rev),
                TargetKind::Pnpm => self.build_pnpm(&target.rev),
                TargetKind::Pnpr => self.build_pnpr(&target.rev),
            }
        }
    }

    fn build_pacquet(&self, revision: &str) {
        let dest = self.pacquet_binary(revision);

        // Restored from the per-commit CI binary cache: nothing to build.
        if self.reuse_prebuilt_binaries && dest.is_file() {
            eprintln!("Revision: {revision:?} (pacquet) — reusing prebuilt binary");
            return;
        }

        // The same commit's `pnpr@<revision>` target (built first) already
        // produced an identical `pacquet` client binary; copy it rather
        // than compiling the revision twice.
        if self
            .targets
            .iter()
            .any(|target| target.kind == TargetKind::Pnpr && target.rev == revision)
        {
            let from_pnpr = self.pnpr_pacquet_binary(revision);
            if from_pnpr.is_file() {
                eprintln!(
                    "Revision: {revision:?} (pacquet) — reusing the binary from the pnpr@{revision} build",
                );
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent).expect("create pacquet target/release dir");
                }
                fs::copy(&from_pnpr, &dest).expect("copy pacquet binary from the pnpr build");
                return;
            }
        }

        eprintln!("Revision: {revision:?} (pacquet)");

        let repository = self.repository();
        let revision_repo = self.pacquet_source_dir(revision);

        // Resolve the revision against the source repository *before*
        // fetching, so the fetch can request the exact commit. A bare
        // `git fetch <repo>` only writes the source's `HEAD` to
        // `FETCH_HEAD`, which means a SHA that isn't reachable from
        // the source's HEAD (e.g. tip of `main` when the runner is on
        // a PR branch that's behind `main`) won't end up in the
        // bench-repo and the subsequent `git checkout <sha>` panics
        // with `unable to read tree`. See PR <https://github.com/pnpm/pacquet/pull/321> comment
        // <https://github.com/pnpm/pacquet/pull/321#issuecomment-4326141435>.
        let commit = WorkEnv::resolve_revision(repository, revision);
        eprintln!("Resolved {revision:?} to {commit}");

        sync_bench_repo(repository, &revision_repo, &commit);

        eprintln!("Building {revision:?}...");
        Command::new("cargo")
            .current_dir(&revision_repo)
            .arg("build")
            .arg("--release")
            .arg("--bin=pacquet")
            .pipe(executor("cargo build"));
    }

    /// Build a pnpr target: both the `pacquet` client and the `pnpr`
    /// server binaries from the revision's monorepo clone. The server is
    /// spawned later, at benchmark time, from
    /// `<bench_dir>/pacquet/target/release/pnpr`.
    fn build_pnpr(&self, revision: &str) {
        // Restored from the per-commit CI binary cache: nothing to build.
        if self.reuse_prebuilt_binaries
            && self.pnpr_pacquet_binary(revision).is_file()
            && self.pnpr_server_binary(revision).is_file()
        {
            eprintln!("Revision: {revision:?} (pnpr) — reusing prebuilt binaries");
            return;
        }

        eprintln!("Revision: {revision:?} (pnpr)");

        let repository = self.repository();
        let revision_repo = self.pnpr_source_dir(revision);

        let commit = WorkEnv::resolve_revision(repository, revision);
        eprintln!("Resolved {revision:?} to {commit}");

        sync_bench_repo(repository, &revision_repo, &commit);

        eprintln!("Building {revision:?} (pacquet + pnpr)...");
        Command::new("cargo")
            .current_dir(&revision_repo)
            .arg("build")
            .arg("--release")
            .arg("--bin=pacquet")
            .arg("--bin=pnpr")
            .pipe(executor("cargo build"));
    }

    fn build_pnpm(&self, revision: &str) {
        eprintln!("Revision: {revision:?} (pnpm)");

        let repository = self.pnpm_repository();
        let revision_repo = self.pnpm_source_dir(revision);

        let commit = WorkEnv::resolve_revision(repository, revision);
        eprintln!("Resolved {revision:?} to {commit}");

        sync_bench_repo(repository, &revision_repo, &commit);

        eprintln!("Installing pnpm deps for {revision:?}...");
        Command::new("pnpm")
            .current_dir(&revision_repo)
            .arg("install")
            .pipe(executor("pnpm install"));

        eprintln!("Compiling pnpm for {revision:?}...");
        // `pnpm run compile-only` rather than `pnpm run compile` —
        // the root `compile` script also runs `update-manifests`,
        // which fires a second `pnpm install` and rewrites tracked
        // manifest files (a no-op for the benchmark, and the
        // rewrite-on-second-run was what made `sync_bench_repo`
        // need its `git reset --hard` guard). `compile-only` keeps
        // the workspace-manifest-reader / typecheck-only setup steps
        // *and* the final `pn -F=pnpm compile` that produces
        // `pnpm/dist/pnpm.{mjs,cjs}` — i.e. everything the install
        // script actually needs.
        Command::new("pnpm")
            .current_dir(&revision_repo)
            .arg("run")
            .arg("compile-only")
            .pipe(executor("pnpm run compile-only"));
    }

    fn benchmark(&self, pnpr_server_registry: &str) {
        let scenario = self.scenario.expect("scenario set when benchmark() is reached");

        // Pre-benchmark wipe of `node_modules`, `store-dir`, and
        // `cache-dir` for every benchmark target, regardless of scenario.
        // The hot-cache scenario's per-iteration `--prepare` intentionally
        // preserves `store-dir` / `cache-dir` so subsequent iterations can
        // reuse them, which means whatever a previous run / scenario /
        // partial invocation left behind would otherwise carry into the
        // warmup — and the warmup wouldn't actually be what primes them.
        // Wiping once upfront makes the warmup the priming run no matter
        // what state the work-env was in. For cold-cache scenarios this is
        // redundant with the per-iteration wipe but harmless (Copilot
        // review on <https://github.com/pnpm/pacquet/pull/296>).
        // `cache-dir` is the client's packument-metadata mirror; wiping it
        // keeps cold-cache scenarios genuinely cold for *resolution*, not
        // just for the CAS. `pnpr-storage` is the per-target pnpr server's
        // store + cache (only present for `pnpr@<rev>` targets) — wiping it
        // upfront (but never per-iteration) makes the hyperfine warmup the
        // run that primes the server, so timed runs measure a warm
        // long-running server even while the client is cold.
        for dir in self.benchmarked_ids().map(|id| self.bench_dir(id)) {
            for name in ["node_modules", "store-dir", "cache-dir", "pnpr-storage"] {
                let path = dir.join(name);
                if path.exists() {
                    fs::remove_dir_all(&path).expect("pre-benchmark wipe");
                }
            }
            let output_log = dir.join(BENCHMARK_OUTPUT_LOG);
            if output_log.exists() {
                fs::remove_file(output_log).expect("pre-benchmark metrics-log wipe");
            }
        }

        // Start a pnpr server per `pnpr@<rev>` target and keep the guards
        // alive for the whole benchmark; they kill the servers on drop at
        // the end of this method. Empty (no-op) when there are no pnpr
        // targets. Spawned before the GVS pre-warm below so a pnpr target
        // would have its server up if a scenario ever combines the two.
        let _pnpr_servers = self.start_pnpr_servers(pnpr_server_registry);

        // For GVS-warm we need a pre-warm pass: hyperfine's `--warmup`
        // would otherwise time-from-empty for the first run since the
        // pre-benchmark wipe above just emptied `store-dir`. The
        // scenario's contract is "GVS already populated", so prime it
        // by running the install once per target before hyperfine
        // starts measuring.
        if scenario.enables_gvs() {
            for id in self.benchmarked_ids() {
                eprintln!("Pre-warming GVS for {id}...");
                Command::new("bash").arg(self.script_path(id)).pipe_mut(executor("install.bash"));
            }
        }

        // hyperfine runs `--prepare` before *each* timed invocation, so
        // cleanup must cover every bench dir we're about to measure.
        // Previously this only wiped the pacquet revisions — if
        // `--with-pnpm` was set, pnpm's `node_modules` survived between
        // iterations, and after the warmup pnpm just hit a no-op
        // "already installed" code path instead of doing real work.
        //
        // Per-iteration cleanup paths come from the scenario: cold-cache
        // scenarios wipe `node_modules` and `store-dir`, hot-cache wipes
        // only `node_modules` so the warmup-populated store survives
        // into the timed runs. Scenarios that mutate `package.json` or
        // the lockfile (the add-dep variant and the no-lockfile install
        // variants) restore a pristine copy saved during `init()` so the
        // next iteration sees the same starting state.
        let cleanup = scenario.cleanup();
        let cleanup_command =
            build_cleanup_command(&cleanup, self.benchmarked_ids(), |id| self.bench_dir(id));

        let mut command = Command::new("hyperfine");
        command.current_dir(self.root()).arg("--prepare").arg(&cleanup_command);

        self.hyperfine_options.append_to(&mut command);

        for id in self.benchmarked_ids() {
            command.arg("--command-name").arg(id.to_string()).arg(self.bash_command(id));
        }

        command
            .arg("--export-json")
            .arg(self.root().join("BENCHMARK_REPORT.json"))
            .arg("--export-markdown")
            .arg(self.root().join("BENCHMARK_REPORT.md"));

        executor("hyperfine")(&mut command);
        self.write_benchmark_diagnostics();
    }

    /// Start a pnpr resolver server for every `pnpr@<rev>`
    /// target and write the `.pnpr-env` its `install.bash` sources. Each
    /// server gets an isolated `<bench_dir>/pnpr-storage`. The returned
    /// guards keep the servers alive and kill them on drop; the vec is
    /// empty when no target is a pnpr target.
    fn start_pnpr_servers(&self, pnpr_server_registry: &str) -> Vec<PnprServer> {
        self.benchmarked_ids()
            .filter(|id| id.is_pnpr())
            .map(|id| self.start_pnpr_server(id, pnpr_server_registry))
            .collect()
    }

    fn start_pnpr_server(&self, id: BenchId, pnpr_server_registry: &str) -> PnprServer {
        let bench_dir = self.bench_dir(id);
        let binary = bench_dir.join("pacquet").join("target").join("release").join("pnpr");
        assert!(
            binary.is_file(),
            "pnpr binary not found at {binary:?} — the build step did not produce it",
        );
        let port = pick_unused_port().expect("pick an unused port for the pnpr server");

        eprintln!("Starting pnpr server for {id} on 127.0.0.1:{port}...");
        let stdout = File::create(bench_dir.join("pnpr-server.stdout.log"))
            .expect("create pnpr server stdout log");
        let stderr = File::create(bench_dir.join("pnpr-server.stderr.log"))
            .expect("create pnpr server stderr log");
        let process = Command::new(&binary)
            .arg("--listen")
            .arg(format!("127.0.0.1:{port}"))
            .arg("--storage")
            .arg(bench_dir.join("pnpr-storage"))
            // The resolver resolves against the registry the client
            // sends, caching packuments in its own store. A long TTL keeps
            // those cached packuments authoritative across the run, the
            // same value the registry-mock pins for the same reason.
            .arg("--packument-ttl-secs")
            .arg("31536000")
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .expect("spawn pnpr server");

        // Wrap the child in its guard *before* anything that can panic
        // (readiness wait, `.pnpr-env` write), so an early failure unwinds
        // through `PnprServer::drop` and kills the process instead of
        // leaking an orphaned server.
        let mut server = PnprServer { process, latency_proxy: None };

        wait_for_pnpr_ready(port);

        // With `--pnpr-latency-ms`, the client reaches the server through
        // a latency-injecting proxy instead of directly, so the benchmark
        // measures pnpr as the remote service it is in production. The
        // proxy guard rides along in `PnprServer` so it's torn down with
        // the server.
        let client_url = if self.pnpr_latency_ms > 0 {
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
        } else {
            format!("http://127.0.0.1:{port}")
        };

        // Must be `PNPM_CONFIG_PNPR_SERVER`, not a bare `PNPR_SERVER`:
        // pacquet reads config env vars only under the `PNPM_CONFIG_*` /
        // `pnpm_config_*` prefix (see `config/src/env_overlay.rs`), so a
        // bare `PNPR_SERVER` is silently ignored and the install runs
        // *direct* instead of through pnpr — making every `pnpr@<rev>`
        // target a duplicate of its `pacquet@<rev>` row.
        // `PACQUET_BENCHMARK_PNPR_TARBALL_REWRITE_FROM` is a source
        // prefix, not the client fetch path. Some registry fixtures return
        // raw upstream tarball URLs even when the server resolves through a
        // latency proxy; the pacquet client rewrites this prefix to its
        // configured registry, which is `client_registry` from `.npmrc`.
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

        server
    }

    pub fn run(&self) {
        // The client registry URL is baked into every target's config
        // during `init`. Direct pacquet/pnpm and the pnpr client tarball
        // materialization go through this URL. The pnpr server receives a
        // separate resolve-registry URL so server-side metadata access can
        // be measured independently.
        let registry_proxy = self.start_client_registry_proxy();
        let client_registry = registry_proxy
            .as_ref()
            .map_or_else(|| self.registry.clone(), |proxy| format!("http://{}/", proxy.addr));
        let pnpr_server_registry_proxy = self.start_pnpr_server_registry_proxy();
        let pnpr_server_registry = pnpr_server_registry_proxy.as_ref().map_or_else(
            || self.registry_cache_populator.clone(),
            |proxy| format!("http://{}/", proxy.addr),
        );

        self.init(&client_registry);
        self.build();
        self.benchmark(&pnpr_server_registry);
        drop(pnpr_server_registry_proxy);
        drop(registry_proxy);
        self.verify_pnpr_targets_were_routed();
        self.verify_benchmark_diagnostics();
    }

    /// Fail the run if a `pnpr@<rev>` target never actually went through its
    /// pnpr server. A resolve populates the server's on-disk store/cache
    /// under `pnpr-storage`, so an empty `pnpr-storage` after the
    /// benchmark means the client resolved *directly* instead — the silent
    /// failure mode where a misnamed `PNPM_CONFIG_PNPR_SERVER` made every
    /// `pnpr@<rev>` row a duplicate of its `pacquet@<rev>` row. Better to
    /// abort than to publish meaningless pnpr-vs-direct numbers.
    fn verify_pnpr_targets_were_routed(&self) {
        for id in self.target_ids().filter(|id| id.is_pnpr()) {
            let storage = self.bench_dir(id).join("pnpr-storage");
            assert!(
                dir_contains_file(&storage),
                "pnpr server storage at {storage:?} is empty after the benchmark — `{id}` never \
                 routed through pnpr (it resolved directly), so its rows would silently duplicate \
                 the `pacquet@<rev>` install. Check that `.pnpr-env` exports \
                 `PNPM_CONFIG_PNPR_SERVER` and that the client reads it.",
            );
        }
    }

    /// Start a proxy in front of an external virtual registry that emulates
    /// a real link (latency + bandwidth cap) for benchmark clients, or
    /// `None` when neither is requested. Spawned Verdaccio registries are
    /// proxied in `main` so their advertised tarball URLs use the proxied
    /// public port.
    fn start_client_registry_proxy(&self) -> Option<LatencyProxy> {
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
    fn start_pnpr_server_registry_proxy(&self) -> Option<LatencyProxy> {
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
    fn registry_for<'a>(&'a self, id: BenchId, client_registry: &'a str) -> &'a str {
        if id.is_proxy_cache_populator() { &self.registry_cache_populator } else { client_registry }
    }

    fn write_benchmark_diagnostics(&self) {
        let diagnostics = self.collect_benchmark_diagnostics();
        let json = serde_json::to_string_pretty(&diagnostics).expect("serialize diagnostics JSON");
        fs::write(self.root().join(BENCHMARK_DIAGNOSTICS_JSON), json)
            .expect("write benchmark diagnostics JSON");
        let markdown = render_diagnostics_markdown(&diagnostics, self.scenario);
        fs::write(self.root().join(BENCHMARK_DIAGNOSTICS_MD), &markdown)
            .expect("write benchmark diagnostics markdown");
    }

    fn collect_benchmark_diagnostics(&self) -> BenchmarkDiagnostics {
        let hyperfine = read_hyperfine_report(&self.root().join("BENCHMARK_REPORT.json"));
        let commands_by_name: HashMap<String, HyperfineCommand> = hyperfine
            .results
            .into_iter()
            .map(|command| (command.name().to_string(), command))
            .collect();
        let targets = self
            .benchmarked_ids()
            .map(|id| {
                let id = id.to_string();
                let phase_events =
                    read_phase_events(&self.root().join(&id).join(BENCHMARK_OUTPUT_LOG));
                let command = commands_by_name.get(&id);
                BenchmarkTargetDiagnostics {
                    id,
                    hyperfine_mean_seconds: command.map(|command| command.mean),
                    phase_summary: summarize_phase_events(&phase_events),
                    phase_events,
                }
            })
            .collect();

        BenchmarkDiagnostics {
            targets,
            pnpr_direct_ratios: collect_pnpr_direct_ratios(&commands_by_name),
        }
    }

    fn verify_benchmark_diagnostics(&self) {
        let diagnostics = read_benchmark_diagnostics(&self.root().join(BENCHMARK_DIAGNOSTICS_JSON));
        self.verify_fresh_pnpr_cold_batch(&diagnostics);
        self.verify_pnpr_direct_ratios(&diagnostics);
    }

    fn verify_fresh_pnpr_cold_batch(&self, diagnostics: &BenchmarkDiagnostics) {
        if self.scenario != Some(BenchmarkScenario::IsolatedFreshInstallColdCacheColdStore) {
            return;
        }
        for target in diagnostics
            .targets
            .iter()
            .filter(|target| requires_fresh_pnpr_cold_batch_metrics(&target.id))
        {
            let Some(partition) = target.phase_summary.partition.as_ref() else {
                panic!(
                    "{id} did not emit create_virtual_store_partition metrics; \
                     benchmark cannot prove the pnpr fresh install exercised the cold batch",
                    id = target.id,
                );
            };
            assert!(
                non_trivial_cold_batch(partition.cold, partition.total),
                "{id} did not exercise a non-trivial cold batch: warm={} cold={} skipped={} total={}",
                partition.warm,
                partition.cold,
                partition.skipped,
                partition.total,
                id = target.id,
            );
        }
    }

    fn verify_pnpr_direct_ratios(&self, diagnostics: &BenchmarkDiagnostics) {
        let Some(scenario) = self.scenario else { return };
        if !scenario.expects_pnpr_not_slower_than_direct() {
            return;
        }
        for ratio in &diagnostics.pnpr_direct_ratios {
            if ratio.revision != "HEAD" {
                continue;
            }
            assert!(
                ratio.ratio <= PNPR_DIRECT_RATIO_MAX,
                "pnpr@{} was slower than pacquet@{}: ratio {:.3} > {:.3} (pnpr {:.3}s, pacquet {:.3}s)",
                ratio.revision,
                ratio.revision,
                ratio.ratio,
                PNPR_DIRECT_RATIO_MAX,
                ratio.pnpr_mean_seconds,
                ratio.pacquet_mean_seconds,
            );
        }
    }
}

#[derive(Debug, Deserialize)]
struct HyperfineReport {
    results: Vec<HyperfineCommand>,
}

#[derive(Debug, Clone, Deserialize)]
struct HyperfineCommand {
    command: String,
    #[serde(default)]
    command_name: Option<String>,
    mean: f64,
}

impl HyperfineCommand {
    fn name(&self) -> &str {
        self.command_name.as_deref().unwrap_or(&self.command)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct BenchmarkDiagnostics {
    targets: Vec<BenchmarkTargetDiagnostics>,
    pnpr_direct_ratios: Vec<PnprDirectRatio>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BenchmarkTargetDiagnostics {
    id: String,
    hyperfine_mean_seconds: Option<f64>,
    phase_summary: PhaseSummary,
    phase_events: Vec<PhaseEvent>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PhaseSummary {
    partition: Option<PartitionMetric>,
    create_virtual_store_mean_ms: Option<f64>,
    link_slots: Vec<LinkSlotsMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PartitionMetric {
    warm: u64,
    cold: u64,
    skipped: u64,
    total: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct LinkSlotsMetric {
    batch: String,
    slots: u64,
    mean_ms: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct PhaseEvent {
    phase: String,
    elapsed_ms: Option<u64>,
    warm: Option<u64>,
    cold: Option<u64>,
    skipped: Option<u64>,
    total: Option<u64>,
    batch: Option<String>,
    slots: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PnprDirectRatio {
    revision: String,
    pnpr_mean_seconds: f64,
    pacquet_mean_seconds: f64,
    ratio: f64,
}

fn read_hyperfine_report(path: &Path) -> HyperfineReport {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read hyperfine report at {}: {err}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("parse hyperfine report at {}: {err}", path.display()))
}

fn read_benchmark_diagnostics(path: &Path) -> BenchmarkDiagnostics {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read benchmark diagnostics at {}: {err}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("parse benchmark diagnostics at {}: {err}", path.display()))
}

fn read_phase_events(path: &Path) -> Vec<PhaseEvent> {
    let Ok(text) = fs::read_to_string(path) else { return Vec::new() };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| {
            value.get("target").and_then(Value::as_str) == Some("pacquet::install::phase")
        })
        .filter_map(|value| {
            let phase = event_str(&value, "phase")?.to_string();
            Some(PhaseEvent {
                phase,
                elapsed_ms: event_u64(&value, "elapsed_ms"),
                warm: event_u64(&value, "warm"),
                cold: event_u64(&value, "cold"),
                skipped: event_u64(&value, "skipped"),
                total: event_u64(&value, "total"),
                batch: event_str(&value, "batch").map(str::to_string),
                slots: event_u64(&value, "slots"),
            })
        })
        .collect()
}

fn event_field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).or_else(|| value.get("fields").and_then(|fields| fields.get(key)))
}

fn event_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    event_field(value, key).and_then(Value::as_str)
}

fn event_u64(value: &Value, key: &str) -> Option<u64> {
    let value = event_field(value, key)?;
    value.as_u64().or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn summarize_phase_events(events: &[PhaseEvent]) -> PhaseSummary {
    let partition =
        events.iter().rev().find(|event| event.phase == "create_virtual_store_partition").and_then(
            |event| {
                Some(PartitionMetric {
                    warm: event.warm?,
                    cold: event.cold?,
                    skipped: event.skipped.unwrap_or(0),
                    total: event.total?,
                })
            },
        );
    let create_virtual_store_mean_ms = mean(
        events
            .iter()
            .filter(|event| event.phase == "create_virtual_store")
            .filter_map(|event| event.elapsed_ms)
            .map(|elapsed| elapsed as f64),
    );
    let link_slots = ["warm", "cold"]
        .into_iter()
        .filter_map(|batch| {
            let matching: Vec<&PhaseEvent> = events
                .iter()
                .filter(|event| {
                    event.phase == "link_slots" && event.batch.as_deref() == Some(batch)
                })
                .collect();
            if matching.is_empty() {
                return None;
            }
            let slots = matching.iter().filter_map(|event| event.slots).max().unwrap_or(0);
            let mean_ms =
                mean(matching.iter().filter_map(|event| event.elapsed_ms).map(|ms| ms as f64))?;
            Some(LinkSlotsMetric { batch: batch.to_string(), slots, mean_ms })
        })
        .collect();
    PhaseSummary { partition, create_virtual_store_mean_ms, link_slots }
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut total = 0.0;
    let mut count = 0_u64;
    for value in values {
        total += value;
        count += 1;
    }
    (count > 0).then_some(total / count as f64)
}

fn collect_pnpr_direct_ratios(
    commands_by_name: &HashMap<String, HyperfineCommand>,
) -> Vec<PnprDirectRatio> {
    let mut ratios: Vec<PnprDirectRatio> = commands_by_name
        .iter()
        .filter_map(|(name, pnpr)| {
            let revision = name.strip_prefix("pnpr@")?;
            let direct = commands_by_name.get(&format!("pacquet@{revision}"))?;
            Some(PnprDirectRatio {
                revision: revision.to_string(),
                pnpr_mean_seconds: pnpr.mean,
                pacquet_mean_seconds: direct.mean,
                ratio: pnpr.mean / direct.mean,
            })
        })
        .collect();
    ratios.sort_by(|a, b| a.revision.cmp(&b.revision));
    ratios
}

fn non_trivial_cold_batch(cold: u64, total: u64) -> bool {
    cold > 0 && (total < 10 || cold.saturating_mul(10) >= total)
}

fn requires_fresh_pnpr_cold_batch_metrics(target_id: &str) -> bool {
    target_id == "pnpr@HEAD"
}

fn render_diagnostics_markdown(
    diagnostics: &BenchmarkDiagnostics,
    scenario: Option<BenchmarkScenario>,
) -> String {
    let mut out = String::from("## Pacquet benchmark diagnostics\n\n");
    if scenario == Some(BenchmarkScenario::IsolatedFreshInstallColdCacheColdStore)
        && contains_uninstrumented_pnpr_main(diagnostics)
    {
        out.push_str(
            "> Note: `pnpr@main` in this no-lockfile cold-store report predates the benchmark tarball URL rewrite, so newly resolved tarballs can use raw loopback registry URLs. `pnpr@HEAD` rewrites those URLs to the client-facing registry and pays the configured registry latency/bandwidth. Treat `pnpr@HEAD / pacquet@HEAD` as the guarded comparison here, not `pnpr@HEAD` versus `pnpr@main`.\n\n",
        );
    }
    out.push_str(
        "| Target | hyperfine mean | warm | cold | skipped | CreateVirtualStore mean | link warm mean | link cold mean |\n",
    );
    out.push_str("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for target in &diagnostics.targets {
        let partition = target.phase_summary.partition.as_ref();
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            target.id,
            format_seconds(target.hyperfine_mean_seconds),
            format_u64(partition.map(|metric| metric.warm)),
            format_u64(partition.map(|metric| metric.cold)),
            format_u64(partition.map(|metric| metric.skipped)),
            format_ms(target.phase_summary.create_virtual_store_mean_ms),
            format_ms(link_slots_mean(&target.phase_summary, "warm")),
            format_ms(link_slots_mean(&target.phase_summary, "cold")),
        );
    }
    if !diagnostics.pnpr_direct_ratios.is_empty() {
        out.push_str("\n| Ratio | value |\n| --- | ---: |\n");
        for ratio in &diagnostics.pnpr_direct_ratios {
            let _ = writeln!(
                out,
                "| pnpr@{} / pacquet@{} | {:.3} |",
                ratio.revision, ratio.revision, ratio.ratio,
            );
        }
    }
    out
}

fn contains_uninstrumented_pnpr_main(diagnostics: &BenchmarkDiagnostics) -> bool {
    diagnostics
        .targets
        .iter()
        .any(|target| target.id == "pnpr@main" && target.phase_summary.partition.is_none())
}

fn link_slots_mean(summary: &PhaseSummary, batch: &str) -> Option<f64> {
    summary.link_slots.iter().find(|metric| metric.batch == batch).map(|metric| metric.mean_ms)
}

fn format_seconds(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_string(), |value| format!("{value:.3}s"))
}

fn format_ms(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_string(), |value| format!("{value:.1}ms"))
}

fn format_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "-".to_string(), |value| value.to_string())
}

/// Whether `dir` contains at least one regular file, recursively. Used to
/// confirm a pnpr server actually wrote something (i.e. served a resolve).
/// A missing/unreadable dir counts as empty.
fn dir_contains_file(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            return true;
        }
        if path.is_dir() && dir_contains_file(&path) {
            return true;
        }
    }
    false
}

/// A pnpr resolver server spawned for one `pnpr@<rev>`
/// target. Killed on drop so it never outlives the benchmark run.
struct PnprServer {
    process: Child,
    /// The latency proxy fronting this server, when `--pnpr-latency-ms`
    /// is set. Dropped (stopping the proxy) alongside the server.
    latency_proxy: Option<LatencyProxy>,
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
fn wait_for_pnpr_ready(port: u16) {
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

/// Fetch `commit` into `revision_repo`, creating it if missing, and
/// check the commit out. Shared between the pacquet and pnpm build
/// paths — both follow the same fetch-by-SHA discipline that PR [#321]
/// established for pacquet revisions.
///
/// [#321]: https://github.com/pnpm/pacquet/pull/321
fn sync_bench_repo(repository: &Path, revision_repo: &Path, commit: &str) {
    // Three entry states for `revision_repo`:
    //   1. doesn't exist          → clone (HEAD set by clone, worktree empty)
    //   2. exists, has `.git`     → reuse: fetch + reset worktree + checkout
    //   3. exists, no `.git`      → init + fetch + checkout (no reset — HEAD
    //                               is unborn until checkout, so `git reset
    //                               --hard` would fatal-error here)
    let had_existing_git = revision_repo.exists() && revision_repo.join(".git").exists();
    if revision_repo.exists() {
        if !had_existing_git {
            eprintln!("Initializing a git repository at {revision_repo:?}...");
            Command::new("git")
                .current_dir(revision_repo)
                .arg("init")
                .arg(revision_repo)
                .arg("--initial-branch=__blank__")
                .pipe(executor("git init"));
        }

        eprintln!("Fetching {commit} from {repository:?}...");
        Command::new("git")
            .current_dir(revision_repo)
            .arg("fetch")
            .arg(repository)
            .arg(commit)
            .pipe(executor("git fetch"));
    } else {
        eprintln!("Cloning {repository:?} to {revision_repo:?}...");
        Command::new("git")
            .arg("clone")
            .arg("--no-checkout")
            .arg(repository)
            .arg(revision_repo)
            .pipe(executor("git clone"));
    }

    if had_existing_git {
        // `pnpm install` and `pnpm run compile-only` from a previous orchestrator
        // run can leave tracked files dirty (e.g. `pnpm-lock.yaml` rewritten,
        // generated `dist/*`). A fresh `git checkout <commit>` against a dirty
        // worktree fails with "Your local changes would be overwritten" — wipe
        // them first.
        eprintln!("Resetting worktree at {revision_repo:?}...");
        Command::new("git")
            .current_dir(revision_repo)
            .arg("reset")
            .arg("--hard")
            .pipe(executor("git reset --hard"));
    }

    eprintln!("Checking out {commit:?}...");
    Command::new("git")
        .current_dir(revision_repo)
        .arg("checkout")
        .arg(commit)
        .pipe(executor("git checkout"));

    eprintln!("List of branches:");
    Command::new("git").current_dir(revision_repo).arg("branch").pipe(executor("git branch"));
}

/// Build the `--prepare` shell command for hyperfine: one `rm -rf`
/// covering every bench dir's removal paths, then a `cp` for each
/// pristine file that needs restoring. Failures abort the iteration
/// via `&&`.
fn build_cleanup_command<'a, Ids, BenchDir>(
    cleanup: &Cleanup,
    ids: Ids,
    mut bench_dir: BenchDir,
) -> String
where
    Ids: Iterator<Item = BenchId<'a>>,
    BenchDir: FnMut(BenchId<'a>) -> PathBuf,
{
    let dirs: Vec<PathBuf> = ids.map(&mut bench_dir).collect();

    let remove_targets = dirs
        .iter()
        .flat_map(|dir| cleanup.remove.iter().map(move |name| dir.join(name)))
        .map(|path| path.maybe_quote().to_string())
        .join(" ");

    let mut command = format!("rm -rf {remove_targets}");

    for dir in &dirs {
        for (dst, src) in cleanup.restore {
            let src_path = dir.join(src).maybe_quote().to_string();
            let dst_path = dir.join(dst).maybe_quote().to_string();
            let _ = write!(command, " && cp {src_path} {dst_path}");
        }
    }

    command
}

fn create_package_json(dst_dir: &Path, src_dir: Option<&Path>) {
    let dst = dst_dir.join("package.json");
    if let Some(src_dir) = src_dir {
        let src = src_dir.join("package.json");
        assert!(src.is_file(), "{src:?} must be a file");
        assert_ne!(src, dst);
        fs::copy(src, dst).expect("copy package.json for the revision");
    } else {
        fs::write(dst, PACKAGE_JSON).expect("write package.json for the revision");
    }
}

/// Save pristine copies of `package.json` and (when present)
/// `pnpm-lock.yaml` next to the originals. The cleanup phase between
/// hyperfine iterations restores from these so a mutating install
/// (`add`, `--no-frozen-lockfile`) doesn't drift the project state
/// across runs.
fn save_pristine_copies(dir: &Path) {
    let pkg = dir.join("package.json");
    if pkg.is_file() {
        fs::copy(&pkg, dir.join(".saved-package.json")).expect("save pristine package.json");
    }
    let lock = dir.join("pnpm-lock.yaml");
    if lock.is_file() {
        fs::copy(&lock, dir.join(".saved-pnpm-lock.yaml")).expect("save pristine pnpm-lock.yaml");
    }
}

/// Synthesize the per-revision `pnpm-workspace.yaml` through a typed
/// [`MinimalWorkspaceManifest`] and emit it via `serde_saphyr`, instead
/// of formatting raw YAML strings. The typed round-trip rules out the
/// `duplicated mapping key` failure modes a string-injection approach
/// is prone to, and keeps the on-disk file in sync with the schema as
/// new fields are added.
///
/// Pacquet's `.npmrc` sets `ignore-scripts=true` so no scripts actually
/// run, but pnpm still warns about `ERR_PNPM_IGNORED_BUILDS` for
/// packages whose postinstalls would have fired — the manifest's
/// `allowBuilds: {core-js: false, es5-ext: false, fsevents: false}`
/// silences those specific warnings and keeps pnpm's output clean so
/// hyperfine doesn't see stderr noise.
///
/// Always guarantees `storeDir: ./store-dir` and `cacheDir: ./cache-dir`
/// end up in the destination. Both pnpm and pacquet read these from this
/// file (pacquet since the `.npmrc` parser explicitly ignores
/// `store-dir`); without them, both fall through to the global default
/// store/cache and the benchmark's per-iteration / pre-benchmark cleanup
/// wipes a directory the install never wrote to. That silently
/// invalidates cold/hot-cache semantics and lets state from previous runs
/// leak in (Copilot review on [#296](https://github.com/pnpm/pacquet/pull/296)).
/// `cacheDir` is the resolution-metadata mirror specifically: keeping it
/// local is what lets the cold-cache scenarios force a real cold resolve.
///
/// If a custom fixture's workspace file already declares `storeDir`,
/// trust it — that's the user opting into a different store layout
/// (e.g. shared store across revisions to test a specific scenario).
/// Only inject our default when the key is absent.
///
/// Also mirrors the `.npmrc` settings (`registry`, `autoInstallPeers`,
/// `ignoreScripts`, `lockfile`) into the workspace file as camelCase
/// keys so pnpm 10 picks them up from either source. Per pnpm's reader
/// (`config/reader/src/index.ts:802-808` at pnpm/pnpm@8eb1be4988),
/// non-camelCase keys in `pnpm-workspace.yaml` are silently dropped, so
/// the npmrc spelling can't be reused verbatim. Pacquet's npmrc parser
/// is the only thing that reads `.npmrc` here; pnpm reads both.
fn create_pnpm_workspace(
    dst_dir: &Path,
    src_dir: Option<&Path>,
    registry: &str,
    scenario: BenchmarkScenario,
) {
    let dst = dst_dir.join("pnpm-workspace.yaml");
    let mut manifest = if let Some(src_dir) = src_dir {
        let src = src_dir.join("pnpm-workspace.yaml");
        if src.is_file() {
            assert_ne!(src, dst);
            let text = fs::read_to_string(&src).expect("read fixture pnpm-workspace.yaml");
            let parsed: MinimalWorkspaceManifest =
                serde_saphyr::from_str(&text).expect("parse fixture pnpm-workspace.yaml");
            if parsed.store_dir.is_none() {
                eprintln!(
                    "warn: fixture's pnpm-workspace.yaml has no top-level `storeDir:` — \
                     injecting `storeDir: ./store-dir` so per-revision store isolation works",
                );
            }
            parsed
        } else {
            MinimalWorkspaceManifest::default_for_benchmark()
        }
    } else {
        MinimalWorkspaceManifest::default_for_benchmark()
    };
    if manifest.store_dir.is_none() {
        manifest.store_dir = Some("./store-dir".to_string());
    }
    // Force the packument-metadata cache bench-local too, for the same
    // per-iteration-wipe reason as `storeDir`. Left at the global default
    // (`~/.cache/pnpm`), the metadata mirror survives every cold-cache
    // wipe, so a direct install resolves from a warm mirror and never pays
    // the packument-fetch waterfall pnpr is built to offload — "cold
    // cache" would then wipe only the CAS, not the resolution cache.
    if manifest.cache_dir.is_none() {
        manifest.cache_dir = Some("./cache-dir".to_string());
    }
    // Pin `packages: ['.']` when the fixture didn't set it. Without this
    // the fresh-resolve install path's project walker
    // (`find_workspace_projects`) defaults to `[".", "**"]` and recurses
    // into the per-revision `<bench_dir>/pacquet/` clone of pnpm/pnpm,
    // tripping on the intentionally malformed test fixture at
    // `workspace/project-manifest-reader/__fixtures__/invalid-package-json/package.json`.
    // The benchmark's installs are always single-project, so restricting
    // to the root is the right scope regardless of fixture.
    if manifest.packages.is_none() {
        manifest.packages = Some(vec![".".to_string()]);
    }
    manifest.registry = Some(registry.to_string());
    manifest.auto_install_peers = Some(true);
    manifest.ignore_scripts = Some(true);
    manifest.lockfile = Some(scenario.lockfile_enabled());
    if scenario.enables_gvs() {
        manifest.enable_global_virtual_store = Some(true);
    }
    let yaml = serde_saphyr::to_string(&manifest).expect("serialize pnpm-workspace.yaml");
    fs::write(dst, yaml).expect("write pnpm-workspace.yaml for the revision");
}

fn create_npmrc(dir: &Path, registry: &str, scenario: BenchmarkScenario) {
    let path = dir.join(".npmrc");
    eprintln!("Creating config file {path:?}...");
    let mut file = File::create(path).expect("create .npmrc");
    writeln!(file, "registry={registry}").unwrap();
    // `store-dir` is read from `pnpm-workspace.yaml` (`storeDir`) by both
    // pnpm and pacquet, not from `.npmrc`. Pacquet's `.npmrc` parser
    // (`crates/npmrc/src/npmrc_auth.rs`) explicitly ignores `store-dir`
    // and a test there pins that behaviour. The static fixture's
    // `storeDir: ./store-dir` already resolves to `{bench_dir}/store-dir`
    // under each per-revision CWD, which gives the same per-revision
    // isolation the redundant `.npmrc` line was supposedly providing.
    writeln!(file, "auto-install-peers=true").unwrap();
    writeln!(file, "ignore-scripts=true").unwrap();
    writeln!(file, "{}", scenario.npmrc_lockfile_setting()).unwrap();
}

fn may_create_lockfile(dst_dir: &Path, scenario: BenchmarkScenario, src_dir: Option<&Path>) {
    let load_lockfile = || -> Cow<'_, str> {
        let Some(src_dir) = src_dir else { return Cow::Borrowed(LOCKFILE) };
        src_dir
            .join("pnpm-lock.yaml")
            .pipe(fs::read_to_string)
            .expect("read fixture lockfile")
            .pipe(Cow::Owned)
    };
    if let Some(lockfile) = scenario.lockfile(load_lockfile) {
        let path = dst_dir.join("pnpm-lock.yaml");
        fs::write(path, lockfile).expect("write pnpm-lock.yaml for the revision");
    }
}

/// Write `install.bash` that invokes `command` (the resolved binary,
/// e.g. `./pacquet/target/release/pacquet` or `node .../pnpm.mjs`)
/// with the scenario's install arguments.
///
/// When `needs_pnpr_env` is set, the script sources `.pnpr-env` (written
/// at benchmark time once the per-target pnpr server has a port) so the
/// client picks up `PNPM_CONFIG_PNPR_SERVER` and routes the install
/// through it. The `source` fails loudly under `errexit` if the file is
/// missing, rather than silently falling back to a direct install.
fn create_install_script(dir: &Path, scenario: BenchmarkScenario, command: &str, id: BenchId) {
    let path = dir.join("install.bash");
    let capture_pacquet_metrics = id.is_pacquet_like();

    eprintln!("Creating script {path:?}...");
    let mut file = File::create(&path).expect("create install.bash");

    writeln!(file, "#!/bin/bash").unwrap();
    writeln!(file, "set -o errexit -o nounset -o pipefail").unwrap();
    writeln!(file, r#"cd "$(dirname "$0")""#).unwrap();
    if id.is_pnpr() {
        writeln!(file, "source ./.pnpr-env").unwrap();
    }
    if capture_pacquet_metrics {
        // pnpm targets cannot emit pacquet phase events, so diagnostics are
        // pacquet/pnpr-only. This adds a small one-sided tracing + file-I/O
        // cost to pnpm comparisons, but keeps materialization regressions
        // visible in the benchmark report.
        writeln!(file, r#"export TRACE="${{TRACE:-pacquet::install::phase=info}}""#).unwrap();
        writeln!(file, r#"export TRACE_FORMAT="${{TRACE_FORMAT:-json}}""#).unwrap();
        writeln!(
            file,
            r#"printf '{{"benchmarkTarget":"{id}","event":"runStart"}}\n' >> {BENCHMARK_OUTPUT_LOG}"#,
        )
        .unwrap();
    }

    write!(file, "exec {command}").unwrap();
    if capture_pacquet_metrics {
        write!(file, " --reporter ndjson").unwrap();
    }
    for arg in scenario.install_args() {
        write!(file, " {arg}").unwrap();
    }
    if capture_pacquet_metrics {
        write!(file, " >> {BENCHMARK_OUTPUT_LOG} 2>&1").unwrap();
    }
    writeln!(file).unwrap();

    make_file_executable(&file).expect("make the script executable");
}

#[derive(Debug, Clone, Copy)]
enum BenchId<'a> {
    PacquetRevision(&'a str),
    PnpmRevision(&'a str),
    PnprRevision(&'a str),
    Static(&'a str),
}

impl<'a> From<&'a TargetSpec> for BenchId<'a> {
    fn from(spec: &'a TargetSpec) -> Self {
        match spec.kind {
            TargetKind::Pacquet => BenchId::PacquetRevision(&spec.rev),
            TargetKind::Pnpm => BenchId::PnpmRevision(&spec.rev),
            TargetKind::Pnpr => BenchId::PnprRevision(&spec.rev),
        }
    }
}

/// Static bench id of the proxy-cache populator — the one id that warms
/// the registry's on-disk cache rather than being benchmarked.
const INIT_PROXY_CACHE_ID: &str = ".init-proxy-cache";

impl BenchId<'_> {
    /// Whether this bench id drives the client through a pnpr server.
    fn is_pnpr(self) -> bool {
        matches!(self, BenchId::PnprRevision(_))
    }

    /// Whether this bench id runs the Rust pacquet client, either
    /// directly or through a pnpr server.
    fn is_pacquet_like(self) -> bool {
        matches!(self, BenchId::PacquetRevision(_) | BenchId::PnprRevision(_))
    }

    /// Whether this is the proxy-cache populator (untimed setup that
    /// warms the registry cache), which always uses the real registry.
    fn is_proxy_cache_populator(self) -> bool {
        matches!(self, BenchId::Static(name) if name == INIT_PROXY_CACHE_ID)
    }
}

impl fmt::Display for BenchId<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BenchId::PacquetRevision(revision) => write!(f, "pacquet@{revision}"),
            BenchId::PnpmRevision(revision) => write!(f, "pnpm@{revision}"),
            BenchId::PnprRevision(revision) => write!(f, "pnpr@{revision}"),
            BenchId::Static(name) => write!(f, "{name}"),
        }
    }
}

#[cfg(test)]
mod tests;
