use clap::{Args, Parser, ValueEnum};
use std::{path::PathBuf, process::Command, str::FromStr};

#[derive(Debug, Parser)]
pub struct CliArgs {
    /// Task to benchmark.
    #[clap(long, short, required_unless_present = "build_only", conflicts_with = "build_only")]
    pub scenario: Option<BenchmarkScenario>,

    /// Port of the local virtual registry. Ignored when `--registry=npm`.
    #[clap(long, short = 'p', default_value_t = 4873)]
    pub registry_port: u16,

    /// Which registry the benchmarked installs hit.
    #[clap(long, value_enum, default_value_t = RegistryMode::Virtual)]
    pub registry: RegistryMode,

    /// Path to the git repository of pacquet.
    #[clap(long, short = 'R', default_value = ".")]
    pub repository: PathBuf,

    /// Path to pnpm's git repository. Only set this if pnpm and pacquet
    /// live in separate clones; defaults to `--repository`, which is
    /// correct for the `pnpm/pnpm` monorepo (where both live together).
    #[clap(long)]
    pub pnpm_repository: Option<PathBuf>,

    /// Override default `package.json` and `pnpm-lock.yaml` by specifying the directory containing them.
    #[clap(long, short = 'D')]
    pub fixture_dir: Option<PathBuf>,

    /// Flags to pass to `hyperfine`.
    #[clap(flatten)]
    pub hyperfine_options: HyperfineOptions,

    /// Path to the work environment.
    #[clap(long, short, default_value = "bench-work-env")]
    pub work_env: PathBuf,

    /// Also benchmark the system-installed pnpm.
    #[clap(long)]
    pub with_pnpm: bool,

    /// Round-trip latency, in milliseconds, to inject between the pacquet
    /// client and the pnpr server, so `pnpr@<rev>` targets are measured
    /// as the remote service pnpr is in production rather than a loopback
    /// peer. Applied as half the value in each direction. `0` disables
    /// injection; non-pnpr targets are unaffected.
    #[clap(long, default_value_t = 0)]
    pub pnpr_latency_ms: u64,

    /// Round-trip latency, in milliseconds, to inject on the client link
    /// to the registry. Direct `pacquet@<rev>` / `pnpm@<rev>` installs and
    /// pnpr clients' tarball fetches use this link. The pnpr server's own
    /// resolution uses `--pnpr-server-registry-latency-ms` instead, so a
    /// benchmark can model a co-located warm server resolving quickly while
    /// remote clients still fetch tarballs across the slower registry link.
    /// `0` disables injection; ignored with `--registry=npm` (already
    /// remote).
    #[clap(long, default_value_t = 0)]
    pub registry_latency_ms: u64,

    /// Round-trip latency, in milliseconds, to inject between each
    /// `pnpr@<rev>` server and the registry it uses for resolution. Keep
    /// this low (often `0`) when modeling production, where pnpr sits near
    /// its registry/cache backend; otherwise the server resolves too slowly
    /// and the benchmark under-represents the client's cold materialization
    /// batch. Direct installs and pnpr clients' tarball fetches are
    /// unaffected; they use `--registry-latency-ms`.
    #[clap(long, default_value_t = 0)]
    pub pnpr_server_registry_latency_ms: u64,

    /// Download-bandwidth cap, in **megabits per second**, on the link to
    /// the client-facing registry, applied to direct installs and pnpr
    /// clients' tarball fetches, so tarballs take the time they would over
    /// a real connection instead of being free on loopback. Loopback serves
    /// at ~GB/s; the public npm registry measured ~190 Mbit/s (~24 MB/s)
    /// peak on a fast link, and typical home/CI links are 50–200 Mbit/s.
    /// Pairs with `--registry-latency-ms` (latency dominates small
    /// packages, bandwidth dominates large ones). `0` leaves the registry
    /// at loopback speed; ignored with
    /// `--registry=npm` (already remote).
    #[clap(long, default_value_t = 0.0)]
    pub registry_bandwidth_mbps: f64,

    /// Model TCP slow start on the client↔registry link: each
    /// connection ramps from a ~14.6 KB initial window toward
    /// `--registry-bandwidth-mbps`, doubling per round trip, instead
    /// of transmitting at the full cap from its first byte. Small and
    /// mid-size tarballs then take the several round trips they cost
    /// over a real link. Requires both `--registry-latency-ms` and
    /// `--registry-bandwidth-mbps` to be set; no effect otherwise.
    #[clap(long)]
    pub registry_slow_start: bool,

    /// Build each target without running the benchmark.
    #[clap(long)]
    pub build_only: bool,

    /// Skip cloning + building a target whose output binary is already
    /// present, e.g. restored from a per-commit CI cache. A `pnpr@<rev>`
    /// build also yields the `pacquet` client binary, so a same-revision
    /// `pacquet@<rev>` reuses it rather than recompiling the commit.
    #[clap(long)]
    pub reuse_prebuilt_binaries: bool,

    /// Targets to benchmark. Each is `pacquet@<rev>`, `pnpm@<rev>`, or
    /// `pnpr@<rev>` (a pacquet client driven through a pnpr server).
    #[clap(required = true)]
    pub targets: Vec<TargetSpec>,
}

/// A benchmark target — a specific revision of pacquet or pnpm to build
/// and measure.
#[derive(Debug, Clone)]
pub struct TargetSpec {
    pub kind: TargetKind,
    pub rev: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Pacquet,
    Pnpm,
    /// A pacquet client driven through a pnpr resolver server.
    /// Builds both the `pacquet` and `pnpr` binaries from the revision's
    /// monorepo clone, boots a per-target pnpr server with an isolated
    /// store, and points the client at it via `PNPR_SERVER`.
    Pnpr,
}

impl FromStr for TargetSpec {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let (prefix, rev) = input.split_once('@').ok_or_else(|| {
            format!("target {input:?}: must be `pacquet@<rev>`, `pnpm@<rev>`, or `pnpr@<rev>`")
        })?;
        let kind = match prefix {
            "pacquet" => TargetKind::Pacquet,
            "pnpm" => TargetKind::Pnpm,
            "pnpr" => TargetKind::Pnpr,
            other => {
                return Err(format!(
                    "target {input:?}: unknown kind {other:?} \
                     (expected `pacquet`, `pnpm`, or `pnpr`)",
                ));
            }
        };
        if rev.is_empty() {
            return Err(format!("target {input:?}: <rev> must not be empty"));
        }
        Ok(TargetSpec { kind, rev: rev.to_string() })
    }
}

/// Where the installs fetch packages from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RegistryMode {
    /// Spawn or attach to a local verdaccio proxy of npmjs.com.
    Verdaccio,
    /// Hit `registry.npmjs.org` directly. No proxy.
    Npm,
    /// Assume an external mock registry on `--registry-port`.
    Virtual,
}

/// Slug shape: `<linker>.<action>.<cache state>.<store state>`. Dots
/// separate the four axes that the bench varies, so charts and
/// dashboards can group by leading segment (`isolated-linker.*`,
/// `gvs-linker.*`, future `hoisted-linker.*` / `pnp-linker.*`).
///
/// Every current variant starts with `node_modules` wiped — "fresh"
/// names that target state; future variants that begin with a
/// populated `node_modules` will use a different action prefix.
//
// Five of six variants share the `Isolated` prefix today; the lint
// will stop firing once the `Hoisted*` and `Pnp*` linker buckets land.
// Keeping the prefix is intentional — it mirrors the slug's leading
// segment and makes the linker grouping legible in code.
#[allow(
    clippy::enum_variant_names,
    reason = "the shared `Isolated` prefix mirrors the scenario slug and keeps the linker grouping legible; it stops firing once other linker buckets land"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BenchmarkScenario {
    /// No lockfile, cold cache + cold store. Mirrors `pnpm install` with nothing on disk.
    #[value(name = "isolated-linker.fresh-install.cold-cache.cold-store")]
    IsolatedFreshInstallColdCacheColdStore,
    /// No lockfile, hot cache + hot store. Resolves everything against an already-populated store.
    #[value(name = "isolated-linker.fresh-install.hot-cache.hot-store")]
    IsolatedFreshInstallHotCacheHotStore,
    /// No lockfile, cold cache + **hot** store. Isolates resolution: the
    /// client must re-resolve the whole tree (cold packument cache → a
    /// fetch waterfall over the registry link) but every tarball is
    /// already in the store, so no download dominates and hides it. This
    /// is the scenario that exposes pnpr's core win — a direct install
    /// pays the full cold resolution while pnpr offloads it to its warm
    /// server. (Direct's `PrefetchingResolver` can't hide the resolution
    /// behind downloads here because there are none.)
    #[value(name = "isolated-linker.fresh-install.cold-cache.hot-store")]
    IsolatedFreshInstallColdCacheHotStore,
    /// Frozen lockfile, cold cache + cold store. The typical CI shape.
    #[value(name = "isolated-linker.fresh-restore.cold-cache.cold-store")]
    IsolatedFreshRestoreColdCacheColdStore,
    /// Frozen lockfile, hot cache + hot store. The repeat-headless-install shape.
    #[value(name = "isolated-linker.fresh-restore.hot-cache.hot-store")]
    IsolatedFreshRestoreHotCacheHotStore,
    /// `pnpm add <dep>` against an existing lockfile, hot cache + hot store.
    #[value(name = "isolated-linker.fresh-add-dep.hot-cache.hot-store")]
    IsolatedFreshAddDepHotCacheHotStore,
    /// Frozen lockfile, hot cache + hot store, `enableGlobalVirtualStore: true` with a pre-warmed GVS.
    #[value(name = "gvs-linker.fresh-restore.hot-cache.hot-store")]
    GvsFreshRestoreHotCacheHotStore,
}

/// Per-iteration cleanup applied by hyperfine's `--prepare`.
pub struct Cleanup {
    /// Paths in the bench dir to `rm -rf` before each iteration.
    pub remove: &'static [&'static str],
    /// `(dst, src)` pairs (relative to the bench dir) to `cp` before
    /// each iteration — restores files the install mutates.
    pub restore: &'static [(&'static str, &'static str)],
}

impl BenchmarkScenario {
    /// Install command arguments. The leading subcommand is the first
    /// element (`install` or `add`), followed by any flags.
    pub fn install_args(self) -> &'static [&'static str] {
        match self {
            BenchmarkScenario::IsolatedFreshInstallColdCacheColdStore
            | BenchmarkScenario::IsolatedFreshInstallHotCacheHotStore
            | BenchmarkScenario::IsolatedFreshInstallColdCacheHotStore => &["install"],
            BenchmarkScenario::IsolatedFreshRestoreColdCacheColdStore
            | BenchmarkScenario::IsolatedFreshRestoreHotCacheHotStore
            | BenchmarkScenario::GvsFreshRestoreHotCacheHotStore => {
                &["install", "--frozen-lockfile"]
            }
            BenchmarkScenario::IsolatedFreshAddDepHotCacheHotStore => &["add", "is-odd"],
        }
    }

    /// Return `lockfile=true` or `lockfile=false` for use in generating `.npmrc`.
    pub fn npmrc_lockfile_setting(self) -> &'static str {
        if self.lockfile_enabled() { "lockfile=true" } else { "lockfile=false" }
    }

    /// Whether the lockfile is enabled for this scenario. Mirrored into
    /// `pnpm-workspace.yaml` alongside `.npmrc` so pnpm picks the same
    /// value up regardless of which config source it prefers.
    pub fn lockfile_enabled(self) -> bool {
        match self {
            BenchmarkScenario::IsolatedFreshInstallColdCacheColdStore
            | BenchmarkScenario::IsolatedFreshInstallHotCacheHotStore
            | BenchmarkScenario::IsolatedFreshInstallColdCacheHotStore => false,
            BenchmarkScenario::IsolatedFreshRestoreColdCacheColdStore
            | BenchmarkScenario::IsolatedFreshRestoreHotCacheHotStore
            | BenchmarkScenario::IsolatedFreshAddDepHotCacheHotStore
            | BenchmarkScenario::GvsFreshRestoreHotCacheHotStore => true,
        }
    }

    /// Whether to seed a `pnpm-lock.yaml` into the bench dir during
    /// init. The two install variants skip this; the restore, add-dep,
    /// and GVS variants need it.
    pub fn lockfile<Text, LoadLockfile>(self, load_lockfile: LoadLockfile) -> Option<String>
    where
        Text: Into<String>,
        LoadLockfile: FnOnce() -> Text,
    {
        self.lockfile_enabled().then(|| load_lockfile().into())
    }

    /// Per-iteration cleanup (paths to remove and saved copies to
    /// restore) applied via hyperfine's `--prepare`.
    pub fn cleanup(self) -> Cleanup {
        const SAVED_LOCKFILE: (&str, &str) = ("pnpm-lock.yaml", ".saved-pnpm-lock.yaml");
        const SAVED_PACKAGE_JSON: (&str, &str) = ("package.json", ".saved-package.json");
        match self {
            BenchmarkScenario::IsolatedFreshInstallColdCacheColdStore => Cleanup {
                // `cache-dir` (the packument-metadata mirror) is wiped
                // alongside `store-dir` so a direct fresh install pays the
                // full cold resolution — fetching every packument over the
                // emulated registry link — which is precisely the cost pnpr
                // offloads to its warm server. Without this the mirror
                // stays warm and direct ≈ pnpr.
                remove: &["node_modules", "pnpm-lock.yaml", "store-dir", "cache-dir"],
                restore: &[SAVED_PACKAGE_JSON],
            },
            BenchmarkScenario::IsolatedFreshRestoreColdCacheColdStore => Cleanup {
                remove: &["node_modules", "store-dir", "cache-dir"],
                restore: &[SAVED_LOCKFILE],
            },
            BenchmarkScenario::IsolatedFreshRestoreHotCacheHotStore => {
                Cleanup { remove: &["node_modules"], restore: &[SAVED_LOCKFILE] }
            }
            BenchmarkScenario::IsolatedFreshAddDepHotCacheHotStore => Cleanup {
                remove: &["node_modules"],
                restore: &[SAVED_LOCKFILE, SAVED_PACKAGE_JSON],
            },
            BenchmarkScenario::IsolatedFreshInstallHotCacheHotStore => Cleanup {
                remove: &["node_modules", "pnpm-lock.yaml"],
                restore: &[SAVED_PACKAGE_JSON],
            },
            // Cold cache (wipe `cache-dir` → re-resolve from scratch) but
            // hot store (keep `store-dir` → no tarball download). Resolution
            // is the only variable cost, so it can't hide behind downloads.
            BenchmarkScenario::IsolatedFreshInstallColdCacheHotStore => Cleanup {
                remove: &["node_modules", "pnpm-lock.yaml", "cache-dir"],
                restore: &[SAVED_PACKAGE_JSON],
            },
            BenchmarkScenario::GvsFreshRestoreHotCacheHotStore => {
                Cleanup { remove: &["node_modules"], restore: &[SAVED_LOCKFILE] }
            }
        }
    }

    /// Whether this scenario requires `enableGlobalVirtualStore: true`
    /// in the workspace manifest, and a pre-warm pass that primes the
    /// store before hyperfine's warmup runs.
    pub fn enables_gvs(self) -> bool {
        matches!(self, BenchmarkScenario::GvsFreshRestoreHotCacheHotStore)
    }

    /// Scenarios where pnpr's server-side resolution is expected to beat
    /// or match a direct pacquet install. Hot-cache scenarios deliberately
    /// skip this canary because there is little resolution work left to
    /// offload and the remote pnpr hop can dominate.
    pub fn expects_pnpr_not_slower_than_direct(self) -> bool {
        matches!(
            self,
            BenchmarkScenario::IsolatedFreshInstallColdCacheColdStore
                | BenchmarkScenario::IsolatedFreshInstallColdCacheHotStore,
        )
    }
}

#[derive(Debug, Args)]
pub struct HyperfineOptions {
    /// Number of warmup runs to perform before the actual measured benchmark.
    #[clap(long, default_value_t = 1)]
    pub warmup: u8,

    /// Minimum number of runs for each command.
    #[clap(long)]
    pub min_runs: Option<u8>,

    /// Maximum number of runs for each command.
    #[clap(long)]
    pub max_runs: Option<u8>,

    /// Exact number of runs for each command.
    #[clap(long)]
    pub runs: Option<u8>,

    /// Print stdout and stderr of the benchmarked program instead of suppressing it
    #[clap(long)]
    show_output: bool,

    /// Ignore non-zero exit codes of the benchmarked program.
    #[clap(long)]
    pub ignore_failure: bool,
}

impl HyperfineOptions {
    pub fn append_to(&self, hyperfine_command: &mut Command) {
        let &HyperfineOptions { show_output, warmup, min_runs, max_runs, runs, ignore_failure } =
            self;
        hyperfine_command.arg("--warmup").arg(warmup.to_string());
        if let Some(min_runs) = min_runs {
            hyperfine_command.arg("--min-runs").arg(min_runs.to_string());
        }
        if let Some(max_runs) = max_runs {
            hyperfine_command.arg("--max-runs").arg(max_runs.to_string());
        }
        if let Some(runs) = runs {
            hyperfine_command.arg("--runs").arg(runs.to_string());
        }
        if show_output {
            hyperfine_command.arg("--show-output");
        }
        if ignore_failure {
            hyperfine_command.arg("--ignore-failure");
        }
    }
}

#[cfg(test)]
mod tests;
