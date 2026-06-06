use crate::{
    cli_args::{
        BenchmarkScenario, Cleanup, HyperfineOptions, RegistryMode, TargetKind, TargetSpec,
    },
    fixtures::{LOCKFILE, PACKAGE_JSON},
    latency_proxy::{LatencyProxy, LinkProfile},
    verify::executor,
    workspace_manifest::MinimalWorkspaceManifest,
};
use itertools::Itertools;
use os_display::Quotable;
use pacquet_fs::file_mode::make_file_executable;
use pacquet_registry_mock::pick_unused_port;
use pipe_trait::Pipe;
use std::{
    borrow::Cow,
    fmt,
    fs::{self, File},
    io::Write,
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

#[derive(Debug)]
pub struct WorkEnv {
    pub root: PathBuf,
    pub with_pnpm: bool,
    pub targets: Vec<TargetSpec>,
    pub registry: String,
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
    /// Download-bandwidth cap (megabits/sec) on the link to the registry,
    /// applied to every client, so tarball fetches cost real time instead
    /// of being free on loopback. `0` leaves the registry at loopback
    /// speed. Ignored in `--registry=npm` mode (already remote).
    pub registry_bandwidth_mbps: f64,
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
            create_install_script(&dir, scenario, &WorkEnv::install_command(id), id.is_pnpr());
            create_npmrc(&dir, registry, scenario);
            may_create_lockfile(&dir, scenario, self.fixture_dir.as_deref());
            save_pristine_copies(&dir);
        }

        if populate_proxy_cache {
            eprintln!("Populating proxy registry cache...");
            Command::new("bash")
                .arg(self.script_path(WorkEnv::INIT_PROXY_CACHE))
                .pipe_mut(executor("install.bash"))
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

    fn benchmark(&self) {
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
        }

        // Start a pnpr server per `pnpr@<rev>` target and keep the guards
        // alive for the whole benchmark; they kill the servers on drop at
        // the end of this method. Empty (no-op) when there are no pnpr
        // targets. Spawned before the GVS pre-warm below so a pnpr target
        // would have its server up if a scenario ever combines the two.
        let _pnpr_servers = self.start_pnpr_servers();

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
    }

    /// Start a pnpr resolver server for every `pnpr@<rev>`
    /// target and write the `.pnpr-env` its `install.bash` sources. Each
    /// server gets an isolated `<bench_dir>/pnpr-storage`. The returned
    /// guards keep the servers alive and kill them on drop; the vec is
    /// empty when no target is a pnpr target.
    fn start_pnpr_servers(&self) -> Vec<PnprServer> {
        self.benchmarked_ids()
            .filter(|id| id.is_pnpr())
            .map(|id| self.start_pnpr_server(id))
            .collect()
    }

    fn start_pnpr_server(&self, id: BenchId) -> PnprServer {
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
        fs::write(
            bench_dir.join(".pnpr-env"),
            format!("export PNPM_CONFIG_PNPR_SERVER={client_url}\n"),
        )
        .expect("write .pnpr-env");

        server
    }

    pub fn run(&self) {
        // Front the registry with the emulated link, when requested. The
        // guard lives for the whole run so installs (in `benchmark`) cross
        // it; the URL is baked into every target's config during `init`.
        // Every client that touches the registry — direct pacquet/pnpm,
        // the pnpr server resolving, and the pnpr client fetching tarballs
        // — goes through it, so the registry-mock is uniformly as remote as
        // the real npm registry (see [`Self::registry_for`]).
        let registry_proxy = self.start_registry_proxy();
        let client_registry = registry_proxy
            .as_ref()
            .map(|proxy| format!("http://{}/", proxy.addr))
            .unwrap_or_else(|| self.registry.clone());

        self.init(&client_registry);
        self.build();
        self.benchmark();
        drop(registry_proxy);
        self.verify_pnpr_targets_were_routed();
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

    /// Start a proxy in front of the registry that emulates a real link
    /// (latency + bandwidth cap) for every client, or `None` when neither
    /// is requested or the registry is already remote (`npm` mode).
    fn start_registry_proxy(&self) -> Option<LatencyProxy> {
        let rate_limit = mbps_to_bytes_per_sec(self.registry_bandwidth_mbps);
        if (self.registry_latency_ms == 0 && rate_limit.is_none())
            || matches!(self.registry_mode, RegistryMode::Npm)
        {
            return None;
        }
        let upstream = SocketAddr::from((Ipv4Addr::LOCALHOST, self.registry_port));
        let profile = LinkProfile {
            one_way: Duration::from_millis(self.registry_latency_ms) / 2,
            rate_limit,
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

    /// The registry a given bench id resolves against. Every benchmarked
    /// target — direct *and* pnpr — uses `client_registry` (the emulated
    /// link when throttling is on; the raw registry otherwise), because a
    /// request to the registry-mock should cost the same regardless of who
    /// makes it. Only the proxy-cache populator keeps the raw (fast) link:
    /// it warms the registry-mock's on-disk cache before timing starts, so
    /// its cost isn't measured and there's no reason to slow it down.
    fn registry_for<'a>(&'a self, id: BenchId, client_registry: &'a str) -> &'a str {
        if id.is_proxy_cache_populator() { &self.registry } else { client_registry }
    }
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

/// Convert a megabits-per-second figure into a bytes-per-second cap for
/// [`LinkProfile`], or `None` for a non-positive value (no cap). 1 Mbit/s
/// = 1_000_000 bits/s = 125_000 bytes/s.
fn mbps_to_bytes_per_sec(mbps: f64) -> Option<u64> {
    (mbps > 0.0).then_some((mbps * 125_000.0) as u64)
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
            command.push_str(&format!(" && cp {src_path} {dst_path}"));
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
fn create_install_script(
    dir: &Path,
    scenario: BenchmarkScenario,
    command: &str,
    needs_pnpr_env: bool,
) {
    let path = dir.join("install.bash");

    eprintln!("Creating script {path:?}...");
    let mut file = File::create(&path).expect("create install.bash");

    writeln!(file, "#!/bin/bash").unwrap();
    writeln!(file, "set -o errexit -o nounset -o pipefail").unwrap();
    writeln!(file, r#"cd "$(dirname "$0")""#).unwrap();
    if needs_pnpr_env {
        writeln!(file, "source ./.pnpr-env").unwrap();
    }

    write!(file, "exec {command}").unwrap();
    for arg in scenario.install_args() {
        write!(file, " {arg}").unwrap();
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

    /// Whether this is the proxy-cache populator (untimed setup that
    /// warms the registry cache), which always uses the real registry.
    fn is_proxy_cache_populator(self) -> bool {
        matches!(self, BenchId::Static(name) if name == INIT_PROXY_CACHE_ID)
    }
}

impl<'a> fmt::Display for BenchId<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BenchId::PacquetRevision(revision) => write!(f, "pacquet@{revision}"),
            BenchId::PnpmRevision(revision) => write!(f, "pnpm@{revision}"),
            BenchId::PnprRevision(revision) => write!(f, "pnpr@{revision}"),
            BenchId::Static(name) => write!(f, "{name}"),
        }
    }
}
