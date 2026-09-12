pub(crate) use fixtures::seed_peer_heavy_registry;

mod scripts;
use scripts::{
    build_cleanup_command, create_install_script, dir_contains_file, may_create_lockfile,
    sync_bench_repo, wipe_bench_dir,
};

mod fixtures;

use fixtures::{create_npmrc, create_package_json, create_pnpm_workspace, save_pristine_copies};

mod server_config;
use server_config::{
    PnprServer, PnprServerPaths, RevisionMockRegistry, append_pnpr_auth_to_npmrc,
    cold_mock_config_yaml, distinct_public_route_registries, mint_pnpr_token, seed_pnpr_auth,
    wait_for_pnpr_ready, write_pnpr_benchmark_config,
};

mod measurements;
use measurements::{
    BenchmarkDiagnostics, BenchmarkTargetDiagnostics, HyperfineCommand, check_peer_heavy_speedup,
    collect_pnpr_direct_ratios, non_trivial_cold_batch, read_benchmark_diagnostics,
    read_hyperfine_report, read_phase_events, render_diagnostics_markdown,
    requires_fresh_pnpr_cold_batch_metrics, summarize_phase_events,
};

mod diagnostics;

mod servers;

mod build;

use crate::{
    cli_args::{BenchmarkScenario, HyperfineOptions, RegistryMode, TargetKind, TargetSpec},
    verify::executor,
};
use os_display::Quotable;
use pipe_trait::Pipe;
use std::{
    collections::HashMap,
    fmt,
    fs::{self},
    path::{Path, PathBuf},
    process::Command,
};

const BENCHMARK_OUTPUT_LOG: &str = "BENCHMARK_OUTPUT.ndjson";
const PREWARM_SCRIPT: &str = "prewarm.bash";
const BENCHMARK_DIAGNOSTICS_JSON: &str = "BENCHMARK_DIAGNOSTICS.json";
const BENCHMARK_DIAGNOSTICS_MD: &str = "BENCHMARK_DIAGNOSTICS.md";
const PNPR_DIRECT_RATIO_MAX: f64 = 1.05;
// Absolute slack for the pnpr-vs-direct gate: the repeat-install
// scenarios finish in tens of milliseconds, where run-to-run noise
// alone can exceed 5% of the mean. A pnpr arm within this many seconds
// of the direct arm passes regardless of the ratio.
const PNPR_DIRECT_ABS_SLACK_SECONDS: f64 = 0.015;
// A pacquet peer-resolution blowup lands *below* 1x on this DAG, so the floor
// only has to clear measurement noise, not encode how far ahead pacquet is.
// The TypeScript resolver's own hot-cache cost moves independently — offline
// resolution of this fixture dropped from ~34s to ~3s in pnpm/pnpm#13504 — and
// a floor tracking that headroom would fail on every TypeScript perf win.
// Compared on each target's fastest run: see `benchmark_target_min`.
const PACQUET_PNPM_SPEEDUP_MIN: f64 = 1.25;
const PNPR_SERVER_REGISTRY_ENV: &str = "PACQUET_BENCHMARK_PNPR_SERVER_REGISTRY";
const PNPR_TARBALL_REWRITE_FROM_ENV: &str = "PACQUET_BENCHMARK_PNPR_TARBALL_REWRITE_FROM";
const PNPR_BENCHMARK_USERNAME: &str = "pnpr-benchmark";
const PNPR_BENCHMARK_PASSWORD: &str = "password123";
const PNPR_BENCHMARK_HTPASSWD: &str =
    "pnpr-benchmark:$2y$04$MpoezfOJSlhn9S4iiOJihue1IMZTZfYclKbajdz.Dt2pvAoBLNAay\n";
const PEER_HEAVY_DEPTH: usize = 5;
const PEER_HEAVY_WIDTH: usize = 80;
const PEER_HEAVY_PROVIDER: &str = "@pnpmtest/peer-benchmark-provider";
const PEER_HEAVY_VERSION: &str = "1.0.0";
const PEER_HEAVY_INTEGRITY: &str = "sha512-z6pI0F3yfz8Zl3R0+g1pwMbdY12h7Osj5UYSTh6Rcpd7UtO7nxeIzf+7gPDOAH4mgxi7BXaTyBDS0hBFgZVssQ==";
const PNPM_BUNDLE_PATHS: [&str; 4] = [
    "pnpm11/pnpm/dist/pnpm.mjs",
    "pnpm11/pnpm/dist/pnpm.cjs",
    "pnpm/dist/pnpm.mjs",
    "pnpm/dist/pnpm.cjs",
];

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

    /// Diagnostic: launch every `pnpr` mock/server with
    /// `RUST_LOG=pnpr::serve_timing=debug` so its per-phase serve timing lands
    /// in the process log. Skews the measured means, so it is for diagnosis only.
    pub serve_timing: bool,
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

    /// The revision this bench id targets, for the revision-keyed kinds
    /// (`pacquet@<rev>` / `pnpr@<rev>`). `None` for static ids and pnpm
    /// targets, which never route to a per-revision mock.
    fn revision(&self) -> Option<&str> {
        match *self {
            BenchId::PacquetRevision(rev) | BenchId::PnprRevision(rev) => Some(rev),
            BenchId::PnpmRevision(_) | BenchId::Static(_) => None,
        }
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

    /// Path of the untimed online priming script, written only for
    /// scenarios with [`BenchmarkScenario::prewarm_install_args`].
    fn prewarm_script_path(&self, id: BenchId) -> PathBuf {
        self.bench_dir(id).join(PREWARM_SCRIPT)
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
                // Revisions disagree on the client bin name (`pnpm` on
                // current checkouts, `pacquet` on older ones); resolve
                // whichever this revision's build produced at script
                // runtime, the same way the pnpm branch below resolves
                // its bundle path.
                let candidates =
                    ["./pacquet/target/release/pnpm", "./pacquet/target/release/pacquet"].join(" ");
                format!(
                    r#""$(for f in {candidates}; do if [ -f "$f" ]; then echo "$f"; break; fi; done)""#,
                )
            }
            BenchId::PnpmRevision(_) => {
                // Take the first bundle that exists: `pnpm.mjs` over
                // `pnpm.cjs`, and the `pnpm11/` layout (where the
                // TypeScript CLI lives) over the pre-move root layout
                // (still produced by revisions that predate the move,
                // e.g. a `pnpm@v10` target). Resolved at script runtime
                // so the existence check sees the bundle produced by
                // `pnpm run compile-only`, not the empty tree visible
                // during `init()`.
                let candidates =
                    PNPM_BUNDLE_PATHS.map(|path| format!("./pnpm-source/{path}")).join(" ");
                format!(
                    r#"node "$(for f in {candidates}; do if [ -f "$f" ]; then echo "$f"; break; fi; done)""#,
                )
            }
            BenchId::Static(_) => "pnpm".to_string(),
        }
    }

    fn init(&self, direct_registry: &str, revision_mocks: &HashMap<String, RevisionMockRegistry>) {
        let scenario = self.scenario.expect("scenario set when init() is reached");
        eprintln!("Initializing...");
        // The proxy-cache populator only runs against a local
        // verdaccio/virtual registry to warm its on-disk cache. With
        // `--registry=npm`, no proxy exists. The peer-heavy fixture is
        // already seeded into hosted storage, so it needs no population
        // pass either.
        let populate_proxy_cache =
            matches!(self.registry_mode, RegistryMode::Verdaccio | RegistryMode::Virtual)
                && !scenario.uses_peer_heavy_fixture();
        let id_list = self
            .target_ids()
            .chain(populate_proxy_cache.then_some(WorkEnv::INIT_PROXY_CACHE))
            .chain(self.with_pnpm.then_some(WorkEnv::SYSTEM_PNPM));
        for id in id_list {
            eprintln!("ID: {id}");
            let dir = self.bench_dir(id);
            let registry = self.registry_for(id, direct_registry, revision_mocks);
            fs::create_dir_all(&dir).expect("create directory for the revision");
            create_package_json(&dir, self.fixture_dir.as_deref(), scenario);
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

    fn benchmark(
        &self,
        pnpr_server_registry: &str,
        revision_mocks: HashMap<String, RevisionMockRegistry>,
    ) {
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
        // long-running server even while the client is cold. `cold-mock-storage`
        // (only the cold-pnpr scenario) is wiped here too so the warmup run
        // starts cold even on a reused work-env, not just the timed iterations.
        for dir in self.benchmarked_ids().map(|id| self.bench_dir(id)) {
            wipe_bench_dir(&dir);
        }

        // Spawn each revision's own tarball-serving mock (see
        // `plan_revision_mocks`). Done after `build()` produced the
        // per-revision binaries and after `init()` warmed the shared storage
        // they serve from; the guards kill the mocks (and their latency
        // proxies) on drop at the end of this method.
        let _revision_mocks = self.start_revision_mocks(revision_mocks);

        // Start a pnpr server per `pnpr@<rev>` target and keep the guards
        // alive for the whole benchmark; they kill the servers on drop at
        // the end of this method. Empty (no-op) when there are no pnpr
        // targets. Spawned before the GVS pre-warm below so a pnpr target
        // would have its server up if a scenario ever combines the two.
        let _pnpr_servers = self.start_pnpr_servers(pnpr_server_registry);

        // For GVS-warm and repeat-install scenarios we need a pre-warm
        // pass: hyperfine's `--warmup` would otherwise time-from-empty
        // for the first run since the pre-benchmark wipe above just
        // emptied `store-dir` (and `node_modules`). Their contracts are
        // "GVS already populated" / "`node_modules` already up to date",
        // so prime them by running the install once per target before
        // hyperfine starts measuring.
        if scenario.enables_gvs() || scenario.prewarms_node_modules() {
            self.prewarm_install_state();
        }

        // hyperfine runs `--prepare` before *each* timed invocation, so
        // cleanup must cover every bench dir we're about to measure.
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

        // Offline scenarios can't let hyperfine's warmup prime the caches —
        // the measured `--offline` command fails against the mirror the
        // pre-benchmark wipe above just emptied — so run the online priming
        // script once per target first. The per-iteration cleanup runs
        // before the priming too: a reused work-env can carry a lockfile or
        // `node_modules` from a previous scenario, and either would let the
        // priming install skip the full resolution that populates the
        // metadata mirror (a locked install fetches tarballs, not
        // packuments; an up-to-date `node_modules` short-circuits the
        // install outright).
        if scenario.prewarm_install_args().is_some() {
            self.prewarm_caches(&cleanup_command);
        }

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
        if scenario.uses_peer_heavy_fixture() {
            self.install_for_lockfile_comparison(&cleanup_command);
        }
        self.write_benchmark_diagnostics();
    }

    /// Prime the install state for the scenarios whose contract is "GVS
    /// already populated" / "`node_modules` already up to date": hyperfine's
    /// `--warmup` would otherwise time-from-empty for the first run, since
    /// the pre-benchmark wipe just emptied `store-dir` and `node_modules`.
    fn prewarm_install_state(&self) {
        for id in self.benchmarked_ids() {
            eprintln!("Pre-warming the install state for {id}...");
            Command::new("bash").arg(self.script_path(id)).pipe_mut(executor("install.bash"));
        }
    }

    /// Offline scenarios can't let hyperfine's warmup prime the caches — the
    /// measured `--offline` command fails against the mirror the
    /// pre-benchmark wipe just emptied — so the online priming script runs
    /// once per target first.
    ///
    /// The per-iteration cleanup runs before the priming too: a reused
    /// work-env can carry a lockfile or `node_modules` from a previous
    /// scenario, and either would let the priming install skip the full
    /// resolution that populates the metadata mirror (a locked install
    /// fetches tarballs, not packuments; an up-to-date `node_modules`
    /// short-circuits the install outright).
    fn prewarm_caches(&self, cleanup_command: &str) {
        Command::new("bash")
            .current_dir(self.root())
            .arg("-c")
            .arg(cleanup_command)
            .pipe_mut(executor("prewarm cleanup"));
        for id in self.benchmarked_ids() {
            eprintln!("Pre-warming caches for {id}...");
            Command::new("bash")
                .arg(self.prewarm_script_path(id))
                .pipe_mut(executor(PREWARM_SCRIPT));
        }
    }

    /// Re-run every target's install from a clean state so the lockfiles they
    /// write can be compared.
    fn install_for_lockfile_comparison(&self, cleanup_command: &str) {
        eprintln!("Verifying peer-heavy lockfile parity...");
        Command::new("bash")
            .current_dir(self.root())
            .arg("-c")
            .arg(cleanup_command)
            .pipe_mut(executor("lockfile comparison cleanup"));
        for id in self.target_ids() {
            Command::new("bash")
                .arg(self.script_path(id))
                .pipe_mut(executor("lockfile comparison install"));
        }
    }
}
impl WorkEnv {
    pub fn run(&self) {
        // The client registry URL is baked into every target's config
        // during `init`. Direct pnpm/pnpm and the pnpr client tarball
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

        let revision_mocks = self.plan_revision_mocks();
        self.init(&client_registry, &revision_mocks);
        self.build();
        self.benchmark(&pnpr_server_registry, revision_mocks);
        drop(pnpr_server_registry_proxy);
        drop(registry_proxy);
        self.verify_pnpr_targets_were_routed();
        self.verify_benchmark_diagnostics();
    }
}
