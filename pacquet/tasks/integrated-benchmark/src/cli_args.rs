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

    /// Path to pnpm's git repository. Required when a target is `pnpm@<rev>`. Defaults to `--repository`.
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

    /// Build each target without running the benchmark.
    #[clap(long)]
    pub build_only: bool,

    /// Targets to benchmark. Each is `pacquet@<rev>` or `pnpm@<rev>`.
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
}

impl FromStr for TargetSpec {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let (prefix, rev) = input
            .split_once('@')
            .ok_or_else(|| format!("target {input:?}: must be `pacquet@<rev>` or `pnpm@<rev>`"))?;
        let kind = match prefix {
            "pacquet" => TargetKind::Pacquet,
            "pnpm" => TargetKind::Pnpm,
            other => {
                return Err(format!(
                    "target {input:?}: unknown kind {other:?} (expected `pacquet` or `pnpm`)",
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

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BenchmarkScenario {
    /// Clean install: no lockfile, cold cache.
    CleanInstall,
    /// Frozen lockfile, cold cache.
    FrozenLockfile,
    /// Frozen lockfile, warm cache.
    FrozenLockfileHotCache,
    /// Re-resolution: add a dep to an existing lockfile, warm cache.
    Peek,
    /// Full resolution without a lockfile, warm cache.
    FullResolution,
    /// GVS warm reinstall: frozen lockfile, warm GVS.
    GvsWarm,
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
            BenchmarkScenario::CleanInstall => &["install"],
            BenchmarkScenario::FrozenLockfile
            | BenchmarkScenario::FrozenLockfileHotCache
            | BenchmarkScenario::GvsWarm => &["install", "--frozen-lockfile"],
            BenchmarkScenario::Peek => &["add", "is-odd"],
            BenchmarkScenario::FullResolution => &["install", "--no-frozen-lockfile"],
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
            BenchmarkScenario::CleanInstall | BenchmarkScenario::FullResolution => false,
            BenchmarkScenario::FrozenLockfile
            | BenchmarkScenario::FrozenLockfileHotCache
            | BenchmarkScenario::Peek
            | BenchmarkScenario::GvsWarm => true,
        }
    }

    /// Whether to seed a `pnpm-lock.yaml` into the bench dir during
    /// init. Scenarios that start without a lockfile (`CleanInstall`,
    /// `FullResolution`) skip this; scenarios that consume a lockfile
    /// or mutate one (`Peek`, the frozen variants, `GvsWarm`) need it.
    pub fn lockfile<Text, LoadLockfile>(self, load_lockfile: LoadLockfile) -> Option<String>
    where
        Text: Into<String>,
        LoadLockfile: FnOnce() -> Text,
    {
        if self.lockfile_enabled() { Some(load_lockfile().into()) } else { None }
    }

    /// Per-iteration cleanup (paths to remove and saved copies to
    /// restore) applied via hyperfine's `--prepare`.
    pub fn cleanup(self) -> Cleanup {
        const SAVED_LOCKFILE: (&str, &str) = ("pnpm-lock.yaml", ".saved-pnpm-lock.yaml");
        const SAVED_PACKAGE_JSON: (&str, &str) = ("package.json", ".saved-package.json");
        match self {
            BenchmarkScenario::CleanInstall => Cleanup {
                remove: &["node_modules", "pnpm-lock.yaml", "store-dir"],
                restore: &[SAVED_PACKAGE_JSON],
            },
            BenchmarkScenario::FrozenLockfile => {
                Cleanup { remove: &["node_modules", "store-dir"], restore: &[SAVED_LOCKFILE] }
            }
            BenchmarkScenario::FrozenLockfileHotCache => {
                Cleanup { remove: &["node_modules"], restore: &[SAVED_LOCKFILE] }
            }
            BenchmarkScenario::Peek => Cleanup {
                remove: &["node_modules"],
                restore: &[SAVED_LOCKFILE, SAVED_PACKAGE_JSON],
            },
            BenchmarkScenario::FullResolution => Cleanup {
                remove: &["node_modules", "pnpm-lock.yaml"],
                restore: &[SAVED_PACKAGE_JSON],
            },
            BenchmarkScenario::GvsWarm => {
                Cleanup { remove: &["node_modules"], restore: &[SAVED_LOCKFILE] }
            }
        }
    }

    /// Whether this scenario requires `enableGlobalVirtualStore: true`
    /// in the workspace manifest, and a pre-warm pass that primes the
    /// store before hyperfine's warmup runs.
    pub fn enables_gvs(self) -> bool {
        matches!(self, BenchmarkScenario::GvsWarm)
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
            hyperfine_command.arg("--ignore-failures");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TargetKind, TargetSpec};
    use std::str::FromStr;

    #[test]
    fn target_spec_pacquet_prefix() {
        let spec = TargetSpec::from_str("pacquet@main").unwrap();
        assert_eq!(spec.kind, TargetKind::Pacquet);
        assert_eq!(spec.rev, "main");
    }

    #[test]
    fn target_spec_pnpm_prefix() {
        let spec = TargetSpec::from_str("pnpm@v9.0.0").unwrap();
        assert_eq!(spec.kind, TargetKind::Pnpm);
        assert_eq!(spec.rev, "v9.0.0");
    }

    #[test]
    fn target_spec_unprefixed_is_rejected() {
        let err = TargetSpec::from_str("HEAD").unwrap_err();
        assert!(err.contains("`pacquet@<rev>` or `pnpm@<rev>`"), "err = {err}");
    }

    #[test]
    fn target_spec_unknown_prefix_is_rejected() {
        let err = TargetSpec::from_str("yarn@main").unwrap_err();
        assert!(err.contains("unknown kind"), "err = {err}");
    }

    #[test]
    fn target_spec_empty_rev_is_rejected() {
        let err = TargetSpec::from_str("pacquet@").unwrap_err();
        assert!(err.contains("<rev> must not be empty"), "err = {err}");
        let err = TargetSpec::from_str("pnpm@").unwrap_err();
        assert!(err.contains("<rev> must not be empty"), "err = {err}");
    }
}
